import { api } from "../bridge/api";
import { Badge, Button, Card, Field, Icon, Toggle } from "../components/ui";
import { useStore } from "../state/store";

export default function SettingsPage() {
  const { config, patchSettings, info, toast } = useStore();
  const s = config?.settings;
  const mobile = !!info?.mobileBuild;
  if (!s) return null;

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>Settings</h1>
        </div>
        {info?.storeBuild && <Badge tone="accent">Store build</Badge>}
      </header>

      <div className="grid-2">
        <Card title="Tunnel">
          {mobile ? (
            <p className="field-hint">
              Device-wide VPN: once connected, all apps on this phone (Wi-Fi and mobile data) go
              through your config. Android asks for VPN permission the first time. Networld itself
              is excluded so its own connection to the server is not looped.
            </p>
          ) : (
            <Field label="Mode" hint="Full device needs administrator rights and the WinTun driver">
              <select value={s.tunnelMode} onChange={(e) => patchSettings({ tunnelMode: e.currentTarget.value as any })}>
                <option value="proxy">Local proxy (SOCKS5 + HTTP)</option>
                <option value="tun">Full device (TUN)</option>
              </select>
            </Field>
          )}
          <div className="grid-2">
            <Field label="SOCKS5 port">
              <input
                type="number"
                value={s.socksPort}
                onChange={(e) => patchSettings({ socksPort: Number(e.currentTarget.value) })}
              />
            </Field>
            <Field label="HTTP proxy port">
              <input
                type="number"
                value={s.httpPort}
                onChange={(e) => patchSettings({ httpPort: Number(e.currentTarget.value) })}
              />
            </Field>
          </div>
          <Toggle
            label="Allow LAN connections"
            hint="Share the proxies with other devices on your network"
            checked={s.allowLan}
            onChange={(v) => patchSettings({ allowLan: v })}
          />
          {!mobile && (
            <Toggle
              label="Set as Windows system proxy"
              checked={s.setSystemProxy}
              onChange={(v) => patchSettings({ setSystemProxy: v })}
            />
          )}
          {s.tunnelMode === "tun" && (
            <div className="grid-2">
              <Field label="Adapter name">
                <input
                  value={s.tunInterfaceName}
                  onChange={(e) => patchSettings({ tunInterfaceName: e.currentTarget.value })}
                />
              </Field>
              <Field label="MTU">
                <input type="number" value={s.tunMtu} onChange={(e) => patchSettings({ tunMtu: Number(e.currentTarget.value) })} />
              </Field>
            </div>
          )}
        </Card>

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
            <Toggle label="Block IPv6" checked={s.blockIpv6} onChange={(v) => patchSettings({ blockIpv6: v })} />
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
                onChange={(e) => patchSettings({ maxReconnectAttempts: Number(e.currentTarget.value) })}
              />
            </Field>
            <Field label="Delay (seconds)">
              <input
                type="number"
                value={s.reconnectDelaySecs}
                onChange={(e) => patchSettings({ reconnectDelaySecs: Number(e.currentTarget.value) })}
              />
            </Field>
          </div>
        </Card>

        <Card title="DNS & UDP">
          <Toggle
            label="Resolve names on the server"
            hint="Stops DNS leaking to your local resolver"
            checked={s.dns.remoteResolve}
            onChange={(v) => patchSettings({ dns: { ...s.dns, remoteResolve: v } })}
          />
          {!mobile && (
          <Field label="DNS servers" hint="Comma separated, used in TUN mode">
            <input
              value={s.dns.servers.join(", ")}
              onChange={(e) =>
                patchSettings({ dns: { ...s.dns, servers: e.currentTarget.value.split(",").map((x) => x.trim()) } })
              }
            />
          </Field>
          )}
          <Toggle
            label="UDP forwarding (UDPGW)"
            hint="Needs badvpn-udpgw on the server"
            checked={s.udpgwEnabled}
            onChange={(v) => patchSettings({ udpgwEnabled: v })}
          />
          {s.udpgwEnabled && (
            <Field label="UDPGW address">
              <input value={s.udpgwAddress} onChange={(e) => patchSettings({ udpgwAddress: e.currentTarget.value })} />
            </Field>
          )}
        </Card>

        <Card title="Diagnostics">
          <Field label="Log level">
            <select value={s.logLevel} onChange={(e) => patchSettings({ logLevel: e.currentTarget.value as any })}>
              {["trace", "debug", "info", "warn", "error"].map((l) => (
                <option key={l} value={l}>
                  {l}
                </option>
              ))}
            </select>
          </Field>
          {!mobile && (
          <div className="row gap mt">
            <Button
              icon="folder"
              onClick={async () => {
                try {
                  await api.openConfigDir();
                } catch (e) {
                  toast("err", String(e));
                }
              }}
            >
              Open config folder
            </Button>
          </div>
          )}
        </Card>
      </div>

      {info && (
        <Card title="About">
          <div className="kv">
            <div>
              <span>Version</span>
              <strong>
                {info.name} {info.version} (core {info.coreVersion})
              </strong>
            </div>
            <div>
              <span>Platform</span>
              <strong>{info.platform}</strong>
            </div>
            {!mobile && (
              <div>
                <span>Package</span>
                <strong>{info.storeBuild ? "Microsoft Store (MSIX)" : "Sideload"}</strong>
              </div>
            )}
            <div>
              <span>Config file</span>
              <strong className="mono tiny">{info.configPath}</strong>
            </div>
            <div>
              <span>Engine binaries</span>
              <strong className="mono tiny">{info.binPath}</strong>
            </div>
          </div>
          <p className="notice">
            <Icon name="shield" size={15} />
            Networld VPN never sends your configurations anywhere. Profiles, payloads and usage counters stay on this
            machine.
          </p>
        </Card>
      )}
    </div>
  );
}
