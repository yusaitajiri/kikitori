import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./en.json";
import ja from "./ja.json";

export const resources = { ja: { translation: ja }, en: { translation: en } } as const;

void i18n.use(initReactI18next).init({
  resources,
  lng: "ja",
  fallbackLng: "en",
  // The spec writes placeholders as {n}.
  interpolation: { escapeValue: false, prefix: "{", suffix: "}" },
  returnNull: false,
});

export default i18n;
