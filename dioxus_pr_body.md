# Fix: Add serde rename for `type` field in SerializedDataTransferItem

## Problem
HTML drag-and-drop events in Dioxus Desktop fail to deserialize due to a field name mismatch. The JavaScript code sends `type` (without underscore), but the Rust struct expects `type_` (with underscore) because `type` is a reserved keyword in Rust.

## Root Cause
The JavaScript serialization code in `dioxus-interpreter-js` sends:
```javascript
items.push({kind:item.kind,type:item.type,data})  // Sends "type"
```

But the Rust struct expects:
```rust
pub struct SerializedDataTransferItem {
    pub kind: String,
    pub type_: String,  // Expects "type_" but JSON has "type"
    pub data: String,
}
```

This causes deserialization to fail with: `missing field 'type_'`

## Solution
Add `#[serde(rename = "type")]` attribute to map the JSON field `type` to the Rust field `type_`:

```rust
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SerializedDataTransferItem {
    pub kind: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub data: String,
}
```

## Testing
- Tested on macOS with Dioxus 0.7.1
- HTML drag-and-drop events now deserialize successfully
- Native file drop handler integration works correctly

## Related
This fix enables HTML drag-and-drop to work properly in desktop applications, allowing the native file drop handler's full paths to be merged with HTML drag events.

