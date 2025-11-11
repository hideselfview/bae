# Fix for Dioxus Desktop Drag-and-Drop Deserialization Error

## Problem
The JavaScript code in `dioxus-interpreter-js` sends `type` (without underscore) in the `SerializedDataTransferItem`, but the Rust struct expects `type_` (with underscore) because `type` is a reserved keyword in Rust.

## Root Cause
In `/Users/dima/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/dioxus-html-0.7.0/src/data_transfer.rs` line 104:

```rust
pub struct SerializedDataTransferItem {
    pub kind: String,
    pub type_: String,  // <-- Expects type_ but JS sends "type"
    pub data: String,
}
```

The JavaScript serialization code sends:
```javascript
items.push({kind:item.kind,type:item.type,data})  // <-- Sends "type"
```

## Solution
Add a `#[serde(rename = "type")]` attribute to map the JSON field `type` to the Rust field `type_`:

```rust
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SerializedDataTransferItem {
    pub kind: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub data: String,
}
```

## Location
File: `dioxus-html/src/data_transfer.rs` (in the dioxus-html crate)
Line: ~104

## Testing
After applying this fix, drag-and-drop events should deserialize correctly and the `ondrop` handler should execute.

