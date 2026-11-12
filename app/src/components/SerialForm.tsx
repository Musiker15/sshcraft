import { useEffect, useState } from "react";
import {
  serialLast,
  serialPorts,
  type FlowControl,
  type Parity,
  type SerialPortInfo,
  type SerialSettings,
} from "../api";
import { useT } from "../i18n/i18n";

const BAUDS = [300, 1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];
const DEFAULTS: SerialSettings = { port: "", baud: 115200, data_bits: 8, stop_bits: 1, parity: "none", flow: "none" };

/** Whether the settings can be opened: a port, a baud rate between 50 and 4 000 000. */
export function serialReady(s: SerialSettings): boolean {
  return s.port.trim() !== "" && Number.isInteger(s.baud) && s.baud >= 50 && s.baud <= 4_000_000;
}

type Props = { onOpen: (settings: SerialSettings) => void };

/** Port, baud rate, data bits, stop bits, parity and flow control; starts from the last used. */
export function SerialForm({ onOpen }: Props) {
  const t = useT();
  const [ports, setPorts] = useState<SerialPortInfo[]>([]);
  const [settings, setSettings] = useState<SerialSettings>(DEFAULTS);
  const [baud, setBaud] = useState(String(DEFAULTS.baud));

  const refresh = () => {
    serialPorts().then(setPorts, () => setPorts([]));
  };
  useEffect(() => {
    refresh();
    serialLast().then(
      (last) => {
        if (last) {
          setSettings(last);
          setBaud(String(last.baud));
        }
      },
      () => {},
    );
  }, []);

  const current: SerialSettings = { ...settings, baud: Number(baud.trim()) };
  const set = (change: Partial<SerialSettings>) => setSettings({ ...settings, ...change });

  return (
    <div className="serial-form">
      <div className="grid-2">
        <label>
          {t("serial.port")}
          <input
            list="serial-ports"
            value={settings.port}
            onChange={(e) => set({ port: e.target.value })}
            aria-label={t("serial.port")}
          />
          <datalist id="serial-ports">
            {ports.map((p) => (
              <option key={p.name} value={p.name}>
                {p.description}
              </option>
            ))}
          </datalist>
        </label>
        <label>
          {t("serial.baud")}
          <input list="serial-bauds" inputMode="numeric" value={baud} onChange={(e) => setBaud(e.target.value)} />
          <datalist id="serial-bauds">
            {BAUDS.map((b) => (
              <option key={b} value={b} />
            ))}
          </datalist>
        </label>
      </div>
      <div className="serial-grid">
        <label>
          {t("serial.dataBits")}
          <select value={settings.data_bits} onChange={(e) => set({ data_bits: Number(e.target.value) })}>
            {[5, 6, 7, 8].map((b) => (
              <option key={b} value={b}>
                {b}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("serial.stopBits")}
          <select value={settings.stop_bits} onChange={(e) => set({ stop_bits: Number(e.target.value) })}>
            <option value={1}>1</option>
            <option value={2}>2</option>
          </select>
        </label>
        <label>
          {t("serial.parity")}
          <select value={settings.parity} onChange={(e) => set({ parity: e.target.value as Parity })}>
            <option value="none">{t("serial.parityNone")}</option>
            <option value="odd">{t("serial.parityOdd")}</option>
            <option value="even">{t("serial.parityEven")}</option>
          </select>
        </label>
        <label>
          {t("serial.flow")}
          <select value={settings.flow} onChange={(e) => set({ flow: e.target.value as FlowControl })}>
            <option value="none">{t("serial.flowNone")}</option>
            <option value="software">{t("serial.flowSoftware")}</option>
            <option value="hardware">{t("serial.flowHardware")}</option>
          </select>
        </label>
      </div>
      <div className="buttons">
        <button onClick={refresh}>{t("serial.refresh")}</button>
        <button className="primary" disabled={!serialReady(current)} onClick={() => onOpen(current)}>
          {t("serial.open")}
        </button>
      </div>
    </div>
  );
}
