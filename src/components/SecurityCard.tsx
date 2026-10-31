import { Card, Field, Toggle } from "./ui";
import { useStore } from "../state/store";

/** Reconnect + safety options (moved here from the old Settings page). */
export default function SecurityCard() {
  const { config, patchSettings, info } = useStore();
  const s = config?.settings;
  const mobile = !!info?.mobileBuild;
  if (!s) return null;

  return (
    <Card title="Security">
      {!mobile && (
        <Toggle
          label="Kill switch"
          hint={
            info?.storeBuild
              ? "Needs administrator rights, so it is not available in the Microsoft Store build"
              : "Default-deny outbound firewall policy while connected"
          }
          disabled={info?.storeBuild}
          checked={s.killSwitch && !info?.storeBuild}
          onChange={(v) => patchSettings({ killSwitch: v })}
        />
      )}
      {!mobile && (
        <Toggle
          label="Block IPv6"
          checked={s.blockIpv6}
          onChange={(v) => patchSettings({ blockIpv6: v })}
        />
      )}
      <Toggle
        label="Auto-reconnect"
        checked={s.autoReconnect}
        onChange={(v) => patchSettings({ autoReconnect: v })}
      />
      <Toggle
        label="Switch config when one fails"
        hint="After 3 failed reconnect attempts, try the next saved config until one stays connected"
        disabled={!s.autoReconnect}
        checked={s.autoSwitchConfig && s.autoReconnect}
        onChange={(v) => patchSettings({ autoSwitchConfig: v })}
      />
      <div className="grid-2">
        <Field label="Max attempts">
          <input
            type="number"
            value={s.maxReconnectAttempts}
            onChange={(e) =>
              patchSettings({
                maxReconnectAttempts: Number(e.currentTarget.value),
              })
            }
          />
        </Field>
        <Field label="Delay (seconds)">
          <input
            type="number"
            value={s.reconnectDelaySecs}
            onChange={(e) =>
              patchSettings({
                reconnectDelaySecs: Number(e.currentTarget.value),
              })
            }
          />
        </Field>
      </div>
    </Card>
  );
}
