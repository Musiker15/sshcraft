import ReactDOM from "react-dom/client";
import App from "./App";
import { SettingsProvider } from "./settings/SettingsProvider";
import "./styles.css";

// No StrictMode: its double mount would open every SSH session twice in development.
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <SettingsProvider>
    <App />
  </SettingsProvider>,
);
