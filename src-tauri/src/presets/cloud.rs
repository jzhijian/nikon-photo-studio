//! Read-only adapter for the public web application's recipe endpoints.
//! No credentials, camera registration, likes, or account mutations are used.
use serde_json::{json, Value};
use serde::Deserialize;
use std::{io::Write, path::Path, time::Duration};

const API: &str = "https://api.user.cwp.imagingcloud.nikon.com/bff/recipe/release";
const MAX_RESPONSE: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum CloudRegion { International, China }

impl CloudRegion {
    fn host(self) -> &'static str {
        match self { Self::International => "imagingcloud.nikon.com", Self::China => "imagingcloud.nikon.com.cn" }
    }
}

fn valid_id(id: &str) -> bool {
    id.len() == 26 && id.bytes().all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b))
}

async fn request(region: CloudRegion, detail: Option<&str>) -> Result<Value, String> {
    if detail.is_some_and(|id| !valid_id(id)) { return Err("无效的云创配方 ID".into()); }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::none()).build()
        .map_err(|_| "无法初始化云创连接")?;
    if let CloudRegion::China = region {
        return match detail {
            None => read_response(client.post("https://api.user.cwp.imagingcloud.nikon.com.cn/bff/recipe/release")
                .json(&json!({"ownerUserId":"nikon", "bookId":"", "sortType":0, "lang_code":"zh-cn", "country":"CN"}))).await,
            Some(id) => {
                // The Chinese frontend resolves the public Nikon owner before requesting a recipe.
                let info = read_response(client.get("https://api.user.ir.imagingcloud.nikon.com.cn/info/userinfo")
                    .header("Accept-Language", "zh-cn")).await?;
                let owner = info["nikonUserId"].as_str().filter(|s| !s.is_empty() && s.len() <= 64
                    && s.bytes().all(|b| b.is_ascii_alphanumeric())).ok_or("中国站缺少公开配方所有者")?;
                let value = read_response(client.get(format!("https://api.user.ir.imagingcloud.nikon.com.cn/publicrecipe/{owner}/{id}?sortType=0&isPagenation=false"))).await?;
                Ok(json!({"recipe":value}))
            }
        };
    }
    let (url, payload) = match detail {
        Some(id) if valid_id(id) => (format!("{API}/detail"), json!({
            "country":"US", "language":"en", "recipeId":id,
            "sortType":"0", "isPagination":"false", "isQueryLang":"false"
        })),
        Some(_) => return Err("无效的云创配方 ID".into()),
        None => (API.into(), json!({
            "ownerUserId":"nikon", "bookId":"", "sortType":0,
            "lang_code":"en", "country":"US"
        })),
    };
    read_response(client.post(url).json(&payload)).await
}

async fn read_response(request: reqwest::RequestBuilder) -> Result<Value, String> {
    let mut response = request.send().await
        .map_err(|_| "无法连接尼康云创，请检查网络后重试")?;
    if !response.status().is_success() {
        return Err(format!("云创返回 HTTP {}，未下载任何预设", response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "云创响应读取中断")? {
        if bytes.len() + chunk.len() > MAX_RESPONSE { return Err("云创响应超过大小限制".into()); }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "云创返回了非 JSON 内容")?;
    if value.get("error_code").and_then(Value::as_str).is_some_and(|s| !s.is_empty()) {
        return Err("云创拒绝了本次公开资料读取，请在官方网站检查".into());
    }
    Ok(value)
}

/// Only keep public descriptive fields. Signed media URLs are intentionally not archived.
fn extract_recipe(value: &Value, expected_id: &str) -> Result<Value, String> {
    let recipe = value.get("recipe").and_then(Value::as_object).ok_or("响应缺少配方")?;
    if recipe.get("id").and_then(Value::as_str) != Some(expected_id)
        || !recipe.get("name").and_then(Value::as_str).is_some_and(|s| !s.trim().is_empty())
        || !recipe.get("pictureControl").and_then(|v| v.get("baseName")).is_some_and(Value::is_string) {
        return Err("云创配方结构已变化，无法验证内容".into());
    }
    let mut result = serde_json::Map::new();
    for key in ["id", "name", "pictureControlName", "pictureControl", "exif", "creatorName", "updated"] {
        if let Some(value) = recipe.get(key) { result.insert(key.into(), value.clone()); }
    }
    Ok(json!({"recipe": result}))
}

#[tauri::command]
pub async fn list_cloud_recipes(region: CloudRegion) -> Result<Value, String> {
    let value = request(region, None).await?;
    let recipes = value.get("recipe").and_then(Value::as_array).ok_or("云创列表结构已变化")?;
    let mut result = Vec::new();
    for recipe in recipes {
        let id = recipe.get("id").and_then(Value::as_str).ok_or("云创列表缺少 ID")?;
        let name = recipe.get("name").and_then(Value::as_str).ok_or("云创列表缺少名称")?;
        if !valid_id(id) { return Err("云创列表包含无效 ID".into()); }
        result.push(json!({"id":id,"name":name}));
    }
    Ok(Value::Array(result))
}

#[tauri::command]
pub async fn get_cloud_recipe(recipe_id: String, region: CloudRegion) -> Result<Value, String> {
    let response = request(region, Some(&recipe_id)).await?;
    extract_recipe(&response, &recipe_id)
}

#[tauri::command]
pub async fn download_cloud_recipe(recipe_id: String, output_path: String, region: CloudRegion) -> Result<String, String> {
    if !output_path.to_lowercase().ends_with(".json") { return Err("公开配方资料必须保存为 JSON，不能伪装为 NP3".into()); }
    let recipe = get_cloud_recipe(recipe_id.clone(), region).await?;
    let document = json!({
        "schemaVersion": 1, "artifactType": "nikon-public-recipe-metadata",
        "sourceUrl": format!("https://{}/recipe/release/detail/?id={recipe_id}", region.host()),
        "downloadedAtUnix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        "executablePictureControl": false, "recipe":recipe["recipe"]
    });
    persist_document(&output_path, &document)?;
    Ok(output_path)
}

fn persist_document(output_path: &str, document: &Value) -> Result<(), String> {
    let path = Path::new(&output_path);
    let parent = path.parent().ok_or("无效的保存路径")?;
    let mut staging = tempfile::NamedTempFile::new_in(parent).map_err(|_| "无法创建下载文件")?;
    staging.write_all(&serde_json::to_vec_pretty(&document).map_err(|_| "配方序列化失败")?)
        .map_err(|_| "写入配方失败")?;
    staging.as_file().sync_all().map_err(|_| "保存配方失败")?;
    staging.persist_noclobber(path).map_err(|_| "目标文件已存在或无法写入，请选择新文件名")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_public_recipe_remains_metadata_and_signed_urls_are_removed() {
        let mut fixture: Value = serde_json::from_str(include_str!("../../../tests/fixtures/cloud-mate-film.json").trim_start_matches('\u{feff}')).unwrap();
        fixture["recipe"]["thumbnail"] = json!("https://example.test/image?Signature=do-not-archive");
        fixture["access_token"] = json!("test-token-do-not-archive");
        let id = fixture["recipe"]["id"].as_str().unwrap();
        let result = extract_recipe(&fixture, id).unwrap();
        assert_eq!(result["recipe"]["pictureControl"]["contrast"], "gamma");
        assert_eq!(result["recipe"]["pictureControl"]["baseName"], "FLEXIBLE COLOR");
        assert!(result["recipe"].get("thumbnail").is_none());
        assert!(result.get("access_token").is_none());
        assert!(extract_recipe(&fixture, "wrong-id").is_err());
        assert!(extract_recipe(&json!({}), id).is_err());
    }
    #[test]
    fn validates_ids_before_network_requests() {
        assert!(valid_id("01M4SD0T78RAGG39JAVXRDR2X0"));
        for id in ["../settings", "", "https://evil.test", "01M4SD0T78RAGG39JAVXRDR2XO"] { assert!(!valid_id(id)); }
    }

    #[test]
    fn archive_does_not_overwrite_existing_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recipe.json").to_string_lossy().into_owned();
        persist_document(&path, &json!({"original":true})).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(persist_document(&path, &json!({"replacement":true})).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    #[ignore = "Explicit live test: contacts Nikon's public service and writes to a temporary directory"]
    async fn live_public_recipe_download() {
        for region in [CloudRegion::International, CloudRegion::China] {
        let catalog = list_cloud_recipes(region).await.unwrap();
        let items = catalog.as_array().unwrap();
        assert!(!items.is_empty());
        let id = items[0]["id"].as_str().unwrap();
        let output = tempfile::tempdir().unwrap();
        let path = output.path().join("recipe.json").to_string_lossy().into_owned();
        download_cloud_recipe(id.into(), path.clone(), region).await.unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["recipe"]["id"], id);
        assert_eq!(saved["executablePictureControl"], false);
        assert!(saved["recipe"]["pictureControl"].is_object());
        assert!(saved["sourceUrl"].as_str().unwrap().starts_with(&format!("https://{}/", region.host())));
        println!("Verified {region:?} live recipe ID: {id}; catalog entries: {}", items.len());
        }
    }
}
