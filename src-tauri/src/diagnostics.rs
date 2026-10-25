//! Headless entry points use the same production commands as the desktop UI.
use crate::presets::cloud::{self, CloudRegion};
use serde_json::{json, Value};

fn region(value: &str) -> Result<CloudRegion, String> {
    match value {
        "china" => Ok(CloudRegion::China),
        "international" => Ok(CloudRegion::International),
        _ => Err("站点必须是 china 或 international".into()),
    }
}

async fn execute(args: &[String]) -> Result<Value, String> {
    match args {
        [operation, sdk, input, output, mode] if operation == "nikon-render" => {
            let mode = match mode.as_str() {
                "as-shot" => crate::library::nikon_sdk::RenderMode::AsShot,
                "saved-edits" => crate::library::nikon_sdk::RenderMode::SavedEdits,
                _ => return Err("显影模式须为 as-shot 或 saved-edits".into()),
            };
            serde_json::to_value(crate::library::nikon_sdk::render_nikon_raw(sdk.clone(), input.clone(), output.clone(), mode).await?).map_err(|e| e.to_string())
        }
        [operation, site] if operation == "cloud-list" => cloud::list_cloud_recipes(region(site)?).await,
        [operation, site, id, path] if operation == "cloud-download" => {
            let path = cloud::download_cloud_recipe(id.clone(), path.clone(), region(site)?).await?;
            Ok(json!({"savedPath":path,"artifactType":"nikon-public-recipe-metadata","executablePictureControl":false}))
        }
        [operation, directory] if operation == "sdk-probe" => crate::camera::sdk_probe::probe_nikon_sdk(directory.clone()).await,
        [operation, path] if operation == "inspect-np3" => {
            let report = crate::presets::commands::inspect_nikon_file(path.clone()).await?;
            serde_json::to_value(report).map_err(|e| e.to_string())
        }
        _ => Err("用法：--diagnostic cloud-list <china|international>；cloud-download <站点> <配方ID> <新JSON路径>；sdk-probe <SDK BinaryFile目录>；inspect-np3 <文件路径>；nikon-render <Image SDK Release目录> <NEF路径> <新PNG路径> <as-shot|saved-edits>".into()),
    }
}

pub fn run_if_requested() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("--diagnostic") { return None; }
    let result = tokio::runtime::Runtime::new().map_err(|e| e.to_string())
        .and_then(|runtime| runtime.block_on(execute(&args[1..])));
    let (output, code) = match result {
        Ok(value) => (value, 0),
        Err(error) => (json!({"error":error}), 1),
    };
    println!("{}", output);
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn invalid_commands_and_regions_do_not_start_work() {
        assert!(execute(&[]).await.is_err());
        assert!(execute(&["cloud-list".into(), "https://example.test".into()]).await.is_err());
        assert!(execute(&["sdk-probe".into()]).await.is_err());
    }
}
