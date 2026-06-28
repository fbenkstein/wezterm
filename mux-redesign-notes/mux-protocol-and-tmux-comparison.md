# Mux Protocol: Wire Format & Comparison with tmux -CC

## Wire format

All messages are framed PDUs. Every frame on the wire is:

```
tagged_len  : leb128 u64   -- byte count of (serial + ident + body)
                            -- bit 63 set = body is zstd-compressed
serial      : leb128 u64   -- request/response correlation ID
ident       : leb128 u64   -- PDU type number
body        : [u8]         -- varbincode-serialized struct
```

All three header fields use unsigned LEB128, so small values cost one byte.
`tagged_len` covers the *encoded* sizes of `serial` and `ident` plus the raw
body length — the decoder subtracts those to get `data_len`.

The body uses **varbincode** (a variable-integer-width variant of bincode,
plugging directly into serde). For payloads larger than 32 bytes the encoder
tries **zstd** compression and uses it only when the result is actually
smaller; bit 63 of `tagged_len` signals which case applies.

The full PDU type table (codec version 45) lives in `codec/src/lib.rs` via
the `pdu!` macro. Unknown idents decode to `Pdu::Invalid { ident }` rather
than erroring, which lets mismatched client/server versions coexist gracefully.
The current codec version is exchanged during handshake via
`GetCodecVersion`/`GetCodecVersionResponse`.

### Transport

The same codec runs over:
- **Unix domain socket** — direct binary stream
- **TLS** — mTLS, with the server issuing client certs via
  `GetTlsCreds`/`GetTlsCredsResponse` during bootstrap
- **SSH stdio tunnel** — binary stream riding over SSH
- **base91** — for carrying the binary stream inside plain-text channels

### Design rationale

The framing layer was written in March 2019 on top of bincode (later switched
to varbincode) because the entire codebase already used serde. Protobuf would
have required a separate schema language and a mapping layer for complex
termwiz types (`Line`, `Cell`, `Hyperlink`, etc.) that don't map cleanly to
proto3. The tradeoff accepted is that the format is not self-describing: an
external client cannot decode it without implementing varbincode.

---

## Comparison with `tmux -CC`

WezTerm implements tmux control mode as a first-class backend (`TmuxDomain`
in `mux/src/tmux.rs`), so a user can attach to an existing tmux session and
get WezTerm rendering. The native mux and tmux -CC differ fundamentally in
what they carry.

### Native mux advantages

**Pre-parsed terminal model.** tmux -CC sends raw VT byte streams in
`%output` events; the client must run a full VT parser. The native mux runs
the parser on the server and sends the already-parsed `Line`/`Cell` model
(`GetLinesResponse`). The client only renders.

**Dirty-line tracking.** `GetPaneRenderChanges` returns only the changed line
ranges, tracked via a per-cell sequence number (`seqno`). tmux -CC has no
equivalent — output is always a full stream.

**Image data transport.** `GetImageCell`/`GetImageCellResponse` carry pixel
data content-addressed by a SHA-256 hash; the client lazily fetches image
blobs it hasn't seen before. tmux intercepts and strips kitty/sixel graphics.

**Input latency correlation.** `SendKeyDown` carries a millisecond timestamp
(`InputSerial`). `GetPaneRenderChangesResponse` echoes it back, allowing
round-trip latency measurement and immediate poll-after-input.

**Richer server-push events.** Dedicated PDUs for `PaneFocused`,
`TabTitleChanged`, `WindowTitleChanged`, `WindowWorkspaceChanged`,
`SetPalette`, `NotifyAlert`, etc. tmux -CC notifies a subset of these and
carries nothing WezTerm-specific.

**mTLS transport.** The server can issue client certificates, enabling a fully
authenticated encrypted channel independent of SSH.

### tmux -CC advantages

**Ubiquity.** tmux is pre-installed on most servers; `wezterm-mux-server`
must be deployed separately.

**Session persistence without WezTerm.** tmux sessions outlive the client
with no daemon required. (The WezTerm mux server also persists sessions, but
it must be running.)

**Multi-client sharing.** Multiple heterogeneous clients can attach to the
same tmux session simultaneously.

**Maturity.** The WezTerm tmux backend still has rough edges: `detach` and
`spawn_pane` are unimplemented, and `spawn` intentionally errors and waits for
async events from the tmux side.

### Summary

The native mux is primarily a *rendering protocol*: it offloads VT parsing to
the server, sends a parsed cell model, and carries WezTerm-specific features
(images, color palettes, hyperlinks) that tmux's text-based control mode
cannot represent. The cost is that both ends must be WezTerm.
