import type { HostKeyPrompt } from "../api";

type Props = { prompt: HostKeyPrompt; onAnswer: (accept: boolean) => void };

export function HostKeyDialog({ prompt, onAnswer }: Props) {
  return (
    <div className="overlay">
      <div className="dialog">
        <h2>Unbekannter Server</h2>
        <p>
          Stollen kennt den Schlüssel von <strong>{prompt.host_name}</strong> ({prompt.address}:{prompt.port}) noch
          nicht. Vergleiche den Fingerprint mit dem des Servers, bevor du ihm vertraust.
        </p>
        <div className="mono">{prompt.fingerprint}</div>
        <div className="buttons">
          <button onClick={() => onAnswer(false)}>Ablehnen</button>
          <button className="primary" onClick={() => onAnswer(true)} autoFocus>
            Vertrauen und verbinden
          </button>
        </div>
      </div>
    </div>
  );
}
