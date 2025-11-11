# Dioxus Desktop Drag-and-Drop Deserialization Error

## Summary
HTML drag-and-drop events in Dioxus Desktop fail to deserialize due to missing `type_` field, causing the `ondrop` handler to never execute.

## Environment
- **Dioxus Version**: 0.7.1
- **Platform**: macOS (darwin 24.5.0)
- **Features**: `["router", "desktop"]`
- **Rust Version**: 1.89.0 (29483883e 2025-08-04)

## Error Message
```
Error parsing user_event: Error("Failed to deserialize event data for event drop: missing field `type_`")
```

## Steps to Reproduce
1. Create a Dioxus desktop app with drag-and-drop handlers:
   ```rust
   div {
       ondragenter: |evt| evt.prevent_default(),
       ondragover: |evt| evt.prevent_default(),
       ondragleave: |evt| evt.prevent_default(),
       ondrop: |evt| {
           evt.prevent_default();
           // This handler never executes due to deserialization error
       },
       "Drop zone"
   }
   ```
2. Drag a folder from Finder onto the drop zone
3. Observe error in console before the `ondrop` handler executes

## Expected Behavior
The `ondrop` handler should execute and be able to access dropped files/folders via `evt.data().data_transfer().files()`.

## Actual Behavior
The event fails to deserialize at the framework level before reaching the handler, with error:
```
missing field `type_`
```

## Error Details
The raw event data structure received from the webview:
```json
{
    "alt_key": false,
    "button": 0,
    "buttons": 0,
    "client_x": 481,
    "client_y": 194,
    "ctrl_key": false,
    "data_transfer": {
        "drop_effect": "none",
        "effect_allowed": "all",
        "files": [
            {
                "content_type": "",
                "last_modified": 1760655961000,
                "name": "1971 IV",
                "path": "1971 IV",
                "size": 0
            }
        ],
        "items": [
            {
                "data": "1971 IV",
                "kind": "file",
                "type": ""
            }
        ]
    },
    "files": [...],
    "meta_key": false,
    "mouse": {...},
    "offset_x": 231,
    "offset_y": 0,
    "page_x": 481,
    "page_y": 194,
    "screen_x": 637,
    "screen_y": 289,
    "shift_key": false
}
```

Note: The event structure is missing a `type_` field that Dioxus expects during deserialization.

## Additional Notes
- Setting `with_disable_drag_drop_handler(true)` in the config does not resolve the issue
- The error occurs at the framework level before any user code executes
- File paths in `data_transfer.files` are relative (just folder name) rather than absolute paths
- This affects HTML drag-and-drop API; native file drop handlers may work differently

## Workaround
Currently, HTML drag-and-drop cannot be used in Dioxus Desktop. Use native file dialogs (e.g., `rfd::AsyncFileDialog`) instead.

## Related
- Dioxus Desktop Config: https://docs.rs/dioxus-desktop/latest/dioxus_desktop/struct.Config.html
- Dioxus Events: https://docs.rs/dioxus/latest/dioxus/events/index.html

