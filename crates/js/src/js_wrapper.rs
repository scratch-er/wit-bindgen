use wit_bindgen_core::{
    uwrite, uwriteln, wit_parser, Files, InterfaceGenerator as _, Source, WorldGenerator,
};
use wit_parser::*;

const WASM_WRAPPER_ENCODE_STR: &str =
r#"
// encode a string into UTF-8 and store it into the WASM linear memory
function wasm_wrapper_encode_str(str) {
    if (typeof str !== "string") {
        throw new TypeError('expected a string');
    }
    if (str.length == 0) {
        return {ptr:1, len:0};
    }
    // encode the string into UTF-8
    let encoded = wasm_wrapper_text_encoder.encode(str);
    let len = encoded.length;
    // allocate memory in the WASM linear memory for the string
    let ptr = wasm_export_realloc(0, 0, 1, len);
    // copy encoded string
    let view = new Uint8Array(wasm_export_memory.buffer, ptr, len);
    view.set(encoded);
    return {ptr, len};
}"#;

const WASM_WRAPPER_DECODE_STR: &str =
r#"
// decode a string stored in the WASM linear memory
function wasm_wrapper_decode_str(ptr, len) {
    let view = new Uint8Array(wasm_export_memory.buffer, ptr, len);
    return wasm_wrapper_text_decoder.decode(view);
}
"#;

const WASM_WRAPPER_LOAD_LIST: &str = 
r#"// load a list from the WASM linear memory
function wasm_wrapper_load_list(ptr, len, type) {
    let ret;
    let view;
    if (type == "S8") {
        ret = new Int8Array(len);
        view = new Int8Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type=="U8" || type=="Bool") {
        ret = new Uint8Array(len);
        view = new Uint8Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "S16") {
        ret = new Int16Array(len);
        view = new Int16Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "U16") {
        ret = new Uint16Array(len);
        view = new Uint16Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "S32") {
        ret = new Int32Array(len);
        view = new Int32Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type=="U32" || type=="Char") {
        ret = new Uint32Array(len);
        view = new Uint32Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "S64") {
        ret = new Int64Array(len);
        view = new Int64Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "U64") {
        ret = new Uint64Array(len);
        view = new Uint64Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "Float32") {
        ret = new Float32Array(len);
        view = new Float32Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "Float64") {
        ret = new Float64Array(len);
        view = new Float64Array(wasm_export_memory.buffer, ptr, len);
    }

    ret.set(view);
    return ret;
}
"#;

const WASM_WRAPPER_STORE_LIST: &str =
r#"
function wasm_wrapper_store_list(lst, type) {
    const len = lst.length;
    let size;
    let align;
    if (type=="U8" || type=="S8" || type=="Bool") {
        size = len;
        align = 1;
    }
    if (type=="U16" || type=="S16") {
        size = len * 2;
        align = 2;
    }
    if (size=="U32" || type=="S32" || type=="Char" || type=="Float32") {
        size = len * 4;
        align = 4;
    }
    if (size=="U64" || type=="S64" || type=="Float64") {
        size = len * 8;
        align = 8;
    }

    ptr = wasm_export_realloc(0, 0, align, len);

    let view;
    if (type == "S8") {
        view = new Int8Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type=="U8" || type=="Bool") {
        view = new Uint8Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "S16") {
        view = new Int16Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "U16") {
        view = new Uint16Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "S32") {
        view = new Int32Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type=="U32" || type=="Char") {
        view = new Uint32Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "S64") {
        view = new Int64Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "U64") {
        view = new Uint64Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "Float32") {
        view = new Float32Array(wasm_export_memory.buffer, ptr, len);
    }
    if (type == "Float64") {
        view = new Float64Array(wasm_export_memory.buffer, ptr, len);
    }

    view.set(lst);
    return {ptr, len};
}
"#;

fn generate_wrapper_function(){}

fn deserialize_wasm_value(){}

fn serialize_wasm_value(){}