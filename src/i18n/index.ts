import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zhCN from "./zh-CN.json";
import enUS from "./en-US.json";

i18n.use(initReactI18next).init({
  resources: {
    "zh-CN": { translation: zhCN },
    "en-US": { translation: enUS },
  },
  lng: navigator.language.startsWith("zh") ? "zh-CN" : "en-US",
  fallbackLng: "en-US",
  interpolation: { escapeValue: false },
});

export default i18n;
