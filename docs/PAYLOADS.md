# Payload injector reference

The payload is the raw byte sequence written to the socket **before** the SSH
handshake. It is how NetMod-style clients get through captive portals,
zero-rated gateways and SNI-filtering middleboxes.

Edit it in **Payload** (live wire preview, token palette, preset loader) or per
server in **Home → ✎ (edit) → Payload (HTTP Request Header)**.

## Tokens

| Token | Expands to |
| --- | --- |
| `[crlf]` | `\r\n` |
| `[cr]` | `\r` |
| `[lf]` | `\n` |
| `[lfcr]` | `\n\r` |
| `[host]` | bug host if set, otherwise the real host |
| `[host_port]` | `[host]:[port]` |
| `[real_host]` | the profile's host, never the bug host |
| `[real_port]` | the profile's port |
| `[ip]` | resolved IP of the real host |
| `[port]` | destination port |
| `[method]` | HTTP method (`CONNECT` by default) |
| `[protocol]` | `HTTP/1.1` |
| `[ua]` | the configured User-Agent |
| `[auth]` | `Proxy-Authorization: Basic …` when proxy credentials exist |
| `[raw]` | `[method] [host_port] [protocol]` |
| `[real_raw]` | `[method] [real_host]:[real_port] [protocol]` |
| `[netData]` | the whole original CONNECT request |
| `[rand:N]` | N random alphanumeric characters |
| `[split]` | flush here and send the rest in a second packet |
| `[delay_split]` | flush, wait the configured split delay, then continue |
| `[delay_N]` | wait N milliseconds at this point |

Unknown `[tokens]` are emitted verbatim, so literal brackets survive.

## Presets

| Preset | Shape |
| --- | --- |
| **Direct** | `[raw][crlf][crlf]` |
| **FrontQuery** | `GET / HTTP/1.1[crlf]Host: [host][crlf][crlf][raw][crlf][crlf]` |
| **BackQuery** | `[raw][crlf]Host: [host][crlf][crlf]GET / HTTP/1.1[crlf][crlf]` |
| **SplitQuery** | `[raw][crlf]Host: [host][crlf][split][crlf]` |
| **DoubleConnect** | two `CONNECT` requests back to back (`expect_responses = 2`) |
| **WebsocketUpgrade** | `GET / HTTP/1.1` + `Upgrade: websocket` + `Sec-WebSocket-Key` |
| **GetHostHeader** | `GET http://[host]/ HTTP/1.1` with an explicit `Host:` |
| **ForwardedHost** | adds `X-Forwarded-Host` / `X-Online-Host` lines |

## Related fields

| Field | Effect |
| --- | --- |
| **Bug host** | value substituted into `[host]`; also the default SNI |
| **SNI** | TLS ServerName sent in the ClientHello — independent of the TCP target |
| **Expect responses** | consume N HTTP response heads before handing the socket to SSH |
| **Accept any status** | ignore the 2xx/101 check (for gateways that answer `400` and still upgrade) |
| **Split delay** | milliseconds used by `[delay_split]` |
| **User-Agent** | fills `[ua]` |

## How it fits together

```
payload non-empty?  ─yes─▶ render template ─▶ write bytes ─▶ consume N responses ─▶ SSH
        │no
        ▼
   SSH Direct  → straight to the SSH handshake
   SSH SSL/TLS → rustls ClientHello with the chosen SNI, then SSH
   SSH Proxy   → CONNECT host:port (+ Basic auth), then SSH
```

TLS and payload compose: with SSH-over-TLS **and** a payload, the payload goes
*inside* the TLS session, right after the handshake.

## Tips

* Always finish an HTTP-shaped payload with a blank line — `[crlf][crlf]`.
* If the gateway answers but SSH never starts, raise **Expect responses** to 2.
* `[split]` beats `[delay_split]` when the middlebox reassembles slowly; use
  `[delay_split]` with 50–200 ms when it inspects only the first packet.
* The preview pane shows `\r` and `\n` explicitly — compare it byte-for-byte
  with a working config from another client when migrating.
