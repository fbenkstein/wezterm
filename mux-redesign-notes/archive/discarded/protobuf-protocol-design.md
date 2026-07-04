# Protobuf Protocol Design

## Update

For the newer side-by-side streaming mux protocol sketch, see
`streaming-mux-protobuf-protocol.md`. This note is still useful background for
wire-format tradeoffs and type-mapping concerns, but it predates the decision
to design a separate protobuf/gRPC-shaped implementation rather than adapt the
current PDU set in place.

## Why consider it

The current codec (LEB128 framing + varbincode body) works but has one hard
constraint: both ends must be WezTerm. There is no schema, no cross-language
tooling, and no field-level backward compatibility — a field added to a struct
changes the binary layout for all fields after it unless varbincode's ordering
happens to be stable. The `CODEC_VERSION` bump-and-hard-fail approach is the
only versioning story.

Protobuf gives:
- A machine-readable schema (`.proto` files) enabling non-WezTerm clients
- Field-number-stable backward compatibility: old clients silently ignore
  unknown fields; new clients handle missing fields via defaults
- Generated code for any language with a protobuf library
- Built-in varint encoding (comparable size to LEB128 for small integers)

## Framing

Protobuf does not define a stream framing format. The current frame layout is
a good model to keep:

```
length  : varint     -- byte count of (body); bit 63 = compressed
serial  : varint     -- request/response correlation ID
ident   : varint     -- message type tag (replaces numeric ident)
body    : bytes      -- serialized protobuf message
```

Alternatively, embed `serial` and the type discriminant inside the protobuf
body itself, and use a simple 4-byte big-endian length prefix. This is
slightly less compact but makes the outer envelope trivially parseable without
a protobuf library.

Compression stays the same: try zstd, use it if smaller, signal via high bit
of `length`.

## Top-level message

The `Pdu` enum maps naturally to a protobuf `oneof`:

```proto
syntax = "proto3";
package wezterm.mux;

message Envelope {
  uint64 serial = 1;
  oneof pdu {
    // Lifecycle
    Ping               ping                = 2;
    Pong               pong                = 3;
    ErrorResponse      error_response      = 4;
    GetCodecVersion    get_codec_version   = 5;
    CodecVersionResp   codec_version_resp  = 6;
    SetClientId        set_client_id       = 7;
    GetClientList      get_client_list     = 8;
    ClientListResp     client_list_resp    = 9;

    // Pane management
    ListPanes          list_panes          = 10;
    ListPanesResp      list_panes_resp     = 11;
    SpawnPane          spawn_pane          = 12;
    SpawnResp          spawn_resp          = 13;
    KillPane           kill_pane           = 14;
    PaneRemoved        pane_removed        = 15;
    SplitPane          split_pane          = 16;
    MovePaneToNewTab   move_pane_new_tab   = 17;
    MovePaneResp       move_pane_resp      = 18;
    Resize             resize              = 19;
    SetPaneZoomed      set_pane_zoomed     = 20;
    ActivatePaneDir    activate_pane_dir   = 21;
    GetPaneDirection   get_pane_direction  = 22;
    PaneDirResp        pane_dir_resp       = 23;
    AdjustPaneSize     adjust_pane_size    = 24;

    // Input
    WriteToPane        write_to_pane       = 30;
    SendKeyDown        send_key_down       = 31;
    SendMouseEvent     send_mouse_event    = 32;
    SendPaste          send_paste          = 33;
    SetFocusedPane     set_focused_pane    = 34;

    // Rendering
    GetPaneChanges     get_pane_changes    = 40;
    PaneChangesResp    pane_changes_resp   = 41;
    GetLines           get_lines           = 42;
    LinesResp          lines_resp          = 43;
    GetPaneDimensions  get_pane_dims       = 44;
    PaneDimsResp       pane_dims_resp      = 45;
    LivenessResponse   liveness_resp       = 46;
    GetImageCell       get_image_cell      = 47;
    ImageCellResp      image_cell_resp     = 48;

    // Server → client notifications
    SetPalette         set_palette         = 50;
    NotifyAlert        notify_alert        = 51;
    PaneFocused        pane_focused        = 52;
    TabResized         tab_resized         = 53;
    TabAddedToWindow   tab_added_to_window = 54;
    TabTitleChanged    tab_title_changed   = 55;
    WindowTitleChanged window_title_changed= 56;
    WindowWorkspace    window_workspace    = 57;
    UnitResponse       unit_response       = 58;

    // Session / workspace
    SetWindowWorkspace set_window_workspace= 60;
    RenameWorkspace    rename_workspace    = 61;
    SetPalette         set_palette         = 62;
    SearchScrollback   search_scrollback   = 63;
    SearchResp         search_resp         = 64;
    EraseScrollback    erase_scrollback    = 65;
  }
}
```

The existing numeric ident table (0–62) can be preserved as the field numbers
inside `oneof`, giving a clean migration path.

## Straightforward type mappings

Most PDU structs are flat and map cleanly:

```proto
message Ping {}
message Pong {}
message UnitResponse {}
message ErrorResponse { string reason = 1; }

message TerminalSize {
  uint32 rows         = 1;
  uint32 cols         = 2;
  uint32 pixel_width  = 3;
  uint32 pixel_height = 4;
  uint32 dpi          = 5;
}

message StableCursorPosition {
  uint64 x          = 1;
  int64  y          = 2;   // StableRowIndex is i64
  uint32 shape      = 3;   // enum CursorShape
  uint32 visibility = 4;   // enum CursorVisibility
}

message RenderableDimensions {
  uint32 cols            = 1;
  uint32 viewport_rows   = 2;
  uint32 scrollback_rows = 3;
  int64  physical_top    = 4;
  int64  scrollback_top  = 5;
  uint32 dpi             = 6;
  uint32 pixel_width     = 7;
  uint32 pixel_height    = 8;
  bool   reverse_video   = 9;
}

message StableRange {
  int64 start = 1;
  int64 end   = 2;
}

message SpawnPane {
  uint64 window_id  = 1;
  string workspace  = 2;
  Command command   = 3;
  TerminalSize size = 4;
}

message Command {
  repeated string argv = 1;
  map<string, string> env = 2;
  optional string cwd = 3;
}

message SpawnResp {
  uint64 tab_id    = 1;
  uint64 pane_id   = 2;
  uint64 window_id = 3;
  TerminalSize size = 4;
}

message Resize {
  uint64 tab_id  = 1;
  uint64 pane_id = 2;
  TerminalSize size = 3;
}
```

## The hard part: Line serialization

`GetLinesResponse` and `GetPaneChangesResponse.bonus_lines` carry
`SerializedLines`, which embeds `termwiz::surface::Line` values — each a
sequence of cells with rich per-cell attributes. This is by far the most
complex mapping.

### Why it's hard

A cell has ~15 independent attribute dimensions: fg/bg color (each a tagged
union of Default/Palette(u8)/TrueColor(rgb)), intensity, underline
style/color, italic, blink, reverse, strikethrough, invisible, overline,
cursor, hyperlink (pointer equality-interned), images (list), semantic type.
Direct cell-per-message encoding is extremely verbose for typical terminal
output (ASCII text with uniform attributes).

### Recommended approach: run-length encoded attribute spans

```proto
message Color {
  oneof value {
    bool    default_color = 1;
    uint32  palette_index = 2;   // 0–255
    uint32  true_color    = 3;   // 0xRRGGBB packed
  }
}

message CellAttrs {
  Color  fg_color      = 1;
  Color  bg_color      = 2;
  Color  underline_color = 3;
  uint32 intensity     = 4;   // enum: Normal/Bold/Half
  uint32 underline     = 5;   // enum: None/Single/Double/Curly/Dotted/Dashed
  bool   italic        = 6;
  uint32 blink         = 7;   // enum
  bool   reverse       = 8;
  bool   strikethrough = 9;
  bool   invisible     = 10;
  bool   overline      = 11;
  uint32 semantic_type = 12;  // enum: Output/Input/Prompt
  uint32 hyperlink_idx = 13;  // 0 = none; index into Line.hyperlinks
  // Images excluded here; carried separately (see SerializedLine.images)
}

message CellRun {
  string    text  = 1;   // UTF-8 content of all cells in this run
  CellAttrs attrs = 2;
  uint32    width = 3;   // number of cells (needed for wide chars)
}

message Hyperlink {
  string uri    = 1;
  map<string, string> params = 2;
}

message ImageCell {
  int64  line_idx      = 1;
  uint32 cell_idx      = 2;
  float  top_left_x    = 3;
  float  top_left_y    = 4;
  float  bottom_right_x = 5;
  float  bottom_right_y = 6;
  bytes  data_hash     = 7;   // 32-byte SHA-256
  int32  z_index       = 8;
  uint32 padding_left  = 9;
  uint32 padding_top   = 10;
  uint32 padding_right = 11;
  uint32 padding_bottom= 12;
  optional uint32 image_id    = 13;
  optional uint32 placement_id = 14;
}

message SerializedLine {
  int64            stable_row = 1;
  repeated CellRun runs       = 2;
  repeated Hyperlink hyperlinks = 3;  // indexed by CellAttrs.hyperlink_idx
  uint64           seqno      = 4;
}

message SerializedLines {
  repeated SerializedLine lines  = 1;
  repeated ImageCell      images = 2;
}
```

The run-length encoding mirrors how the current `Line` stores cells
internally, and matches what SSH-era terminal protocols (like those in
libvterm) use. Typical terminal output — a screen of 80-column text with
uniform attributes — compresses to roughly one `CellRun` per logical line
segment with changed attributes.

### Alternative: opaque bytes

Since both ends are WezTerm anyway, you could serialize `Line` values as
opaque `bytes` using the existing varbincode encoding and embed them in a
proto field. This keeps the hard part unchanged while getting proto's
versioning benefits on everything else. It's a pragmatic intermediate step.

## Recursive type: PaneNode

`PaneNode` is a binary tree (Empty | Split { left, right, node } | Leaf).
Proto3 supports recursive messages via explicit `message` nesting:

```proto
message PaneNode {
  oneof node {
    Empty    empty = 1;
    Split    split = 2;
    PaneEntry leaf = 3;
  }
}

message Empty {}

message Split {
  PaneNode             left  = 1;
  PaneNode             right = 2;
  SplitDirectionAndSize node  = 3;
}

message SplitDirectionAndSize {
  uint32       direction = 1;  // enum: Horizontal/Vertical
  TerminalSize first     = 2;
  TerminalSize second    = 3;
}

message PaneEntry {
  uint64               window_id      = 1;
  uint64               tab_id         = 2;
  uint64               pane_id        = 3;
  string               title          = 4;
  TerminalSize         size           = 5;
  optional string      working_dir    = 6;
  bool                 is_active      = 7;
  bool                 is_zoomed      = 8;
  string               workspace      = 9;
  StableCursorPosition cursor_pos     = 10;
  int64                physical_top   = 11;
  uint32               top_row        = 12;
  uint32               left_col       = 13;
}
```

## ColorPalette

The current `SetPalette` carries a full `ColorPalette`. A palette is 256
ANSI entries plus ~10 named semantic colors (foreground, background, cursor,
selection, etc.):

```proto
message RgbColor {
  uint32 red   = 1;
  uint32 green = 2;
  uint32 blue  = 3;
}

message ColorPalette {
  repeated RgbColor colors     = 1;  // exactly 256 entries
  RgbColor foreground          = 2;
  RgbColor background          = 3;
  RgbColor cursor_fg           = 4;
  RgbColor cursor_bg           = 5;
  RgbColor cursor_border       = 6;
  RgbColor selection_fg        = 7;
  RgbColor selection_bg        = 8;
  repeated RgbColor tab_bar    = 9;
  RgbColor scrollbar_thumb     = 10;
  RgbColor split               = 11;
  RgbColor compose_cursor      = 12;
}
```

## What you gain

**Backward compatibility without CODEC_VERSION hard-fails.** Adding a new
field to any message is invisible to old clients. Removing a field is safe
as long as clients handle the zero/empty default. The current approach
requires both ends to be rebuilt together.

**Schema as documentation.** The `.proto` files become the authoritative
spec for the protocol. Today, the spec is the Rust structs.

**Cross-language clients.** A Python script or Go tool could implement a mux
client without reimplementing varbincode.

**Tooling.** `protoc` plugins, grpcurl-style inspection, Wireshark dissectors.

## What you lose or complicate

**The opaque `bytes`-in-varbincode trick for `Line`.** Currently `Line` is
serialized by serde without any explicit protocol contract. Proto forces an
explicit schema, which is actually better long term but is the largest
up-front cost.

**Compression is per-PDU today.** That stays the same with the framing
approach above. With gRPC (if ever adopted) you'd get stream-level
compression instead.

**Slightly larger messages for very small PDUs.** A proto `Ping {}` encodes
to 0 bytes of body (empty message); the current varbincode encoding is also
0 bytes. For very simple messages there's parity. For structured messages
with many small integers, proto varints are comparable to varbincode.

## Migration path

1. Add proto encoding as an alternative body encoding, signalled by a new
   bit in the frame header (e.g., bit 62 of `length`, alongside the existing
   compression bit 63). Both ends advertise support in
   `GetCodecVersionResponse`.
2. Implement `Line` serialization as opaque varbincode bytes initially
   (the pragmatic intermediate step described above).
3. Migrate `Line` to the `SerializedLine` proto schema once the rest is
   stable.
4. Drop the varbincode path once all supported versions speak proto.
