import ReactDOM from "react-dom/client";
import App from "./App";
import { I18nProvider } from "./i18n/i18n";
import { resolveLanguage } from "./i18n/language";
import "./styles.css";

// No StrictMode: its double mount would open every SSH session twice in development.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  // Provisional until the settings provider exists: follow the system language.
  <I18nProvider language={resolveLanguage("auto", navigator.language)}>
    <App />
  </I18nProvider>,
);
