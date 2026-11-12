import { SettingsProvider } from "./settings/SettingsProvider";
import { AppShell } from "./shell/AppShell";

export default function App() {
  return (
    <SettingsProvider>
      <AppShell />
    </SettingsProvider>
  );
}
