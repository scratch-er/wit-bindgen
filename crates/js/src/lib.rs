mod js_wrapper;

use heck::*;
use std::collections::HashMap;
use std::fmt::Write;
use wit_bindgen_core::{
    uwrite, uwriteln, wit_parser, Files, Source, WorldGenerator,
};
use wit_parser::*;
use js_wrapper::*;

#[derive(Default)]
struct Js {
    src: Source,
    opts: Opts,
    sizes: SizeAlign,
}

#[derive(Default, Debug, Clone)]
#[cfg_attr(feature = "clap", derive(clap::Args))]
pub struct Opts {
    /// Generate code for QuickJS
    ///
    /// If this option is enabled, the generated JS module will use
    /// the `std` module of QuickJS to load wasm file from filesystem
    /// instead of `fetch`, which is incomplete in QuickJS.
    #[cfg_attr(feature = "clap", arg(long))]
    qjs: bool,
    /// Generate code for node.js
    ///
    /// If this option is enabled, the generated JS module will use
    /// the `fs` module of node.js to load wasm file from filesystem
    /// instead of `fetch`.
    #[cfg_attr(feature = "clap", arg(long))]
    node: bool,
}

impl Opts {
    pub fn build(&self) -> Box<dyn WorldGenerator> {
        let mut r = Js::default();
        r.opts = self.clone();
        Box::new(r)
    }
}

impl WorldGenerator for Js {
    fn preprocess(&mut self, resolve: &Resolve, world: WorldId) {
        self.sizes.fill(resolve);

        let world = &resolve.worlds[world];


        uwriteln!(
            self.src,
            "// Fetch and compile the module"
        );

        if self.opts.qjs && self.opts.node {
            panic!("--node conflicts with --qjs");
        } else if self.opts.node {
            uwriteln!(
                self.src,
                r#"const node_module_fs = await import("fs");
                const wasm_module_binary = await node_module_fs.readFileSync("./{}.wasm");
                const wasm_module = new WebAssembly.Module(wasm_module_binary);
                "#,
                world.name
            );
        } else if self.opts.qjs {
            uwriteln!(
                self.src,
                r#"import * as std from "std"

                function loadFile(filename) {{
                    const file = std.open(filename, "rb")
                    file.seek(0, std.SEEK_END)
                    const file_size = file.tell()
                    file.seek(0, std.SEEK_SET)
                    const binary = new ArrayBuffer(file_size);
                    file.read(binary, 0, file_size)
                    file.close();
                    return binary;
                }}
                const wasm_module = new WebAssembly.Module(loadFile("{}.wasm"));
                "#,
                world.name
            );
        } else {
            uwriteln!(
                self.src,
                r#"const wasm_binary_res = await fetch(new URL("./{}.wasm", import.meta.url));
                const wasm_module_binary = await wasm_binary_res.arrayBuffer();
                const wasm_module = new WebAssembly.Module(wasm_module_binary);
                "#,
                world.name
            );
        }

        uwriteln!(self.src,
            "// Construct the import object"
        );
        uwriteln!(self.src, "let wasm_import_objects = {{}};");
    }

    fn import_interface(
        &mut self,
        resolve: &Resolve,
        name: &WorldKey,
        id: InterfaceId,
        _files: &mut Files,
    ) {
        let iface_name = resolve.name_world_key(name);
        let iface = &resolve.interfaces[id];
        let funcs = &iface.functions;
        uwrite!(self.src, "import {{");
        for (func_name, _func) in funcs {
            uwrite!(self.src,
                "{} as wasm_import_{}_{}, ",
                func_name.to_lower_camel_case(),
                iface_name.to_lower_camel_case(),
                func_name.to_lower_camel_case()
            );
        }
        uwriteln!(self.src, r#"}} from "./{}.js";"#, iface_name);
        uwriteln!(self.src,
            r#"wasm_import_objects["{}"] = {{}};"#,
            iface_name
        );
        for (func_name, _func) in funcs {
            uwriteln!(self.src,
                r#"wasm_import_objects["{}"]["{}"] = wasm_import_{}_{};"#,
                iface_name, func_name, iface_name.to_lower_camel_case(),
                func_name.to_lower_camel_case()
            );
        }
    }

    fn import_funcs(
        &mut self,
        _resolve: &Resolve,
        _world: WorldId,
        funcs: &[(&str, &Function)],
        _files: &mut Files,
    ) {
        uwriteln!(self.src, "\n// import functions");
        uwrite!(self.src, "import {{");
        for (func_name, _func) in funcs {
            uwrite!(
                self.src,
                "{} as wasm_import_root_function_{}, ",
                func_name.to_lower_camel_case(),
                func_name.to_lower_camel_case()
            );
        }
        uwriteln!(self.src, r#"}} from "./root.js""#);

        uwriteln!(self.src, r#"wasm_import_objects["$root"] = {{}};"#);
        for (func_name, _func) in funcs {
            uwriteln!(self.src,
                r#"wasm_import_objects["$root"]["{}"] = wasm_import_root_function_{};"#,
                func_name,
                func_name.to_lower_camel_case()
            );
        }
    }

    fn export_interface(
        &mut self,
        resolve: &Resolve,
        name: &WorldKey,
        id: InterfaceId,
        _files: &mut Files,
    ) {
        todo!("export_interface() not implemented");
    }

    fn export_funcs(
        &mut self,
        _resolve: &Resolve,
        _world: WorldId,
        funcs: &[(&str, &Function)],
        _files: &mut Files,
    ) {
        uwriteln!(
            self.src,
            r#"
            // Instantiate the module
            let wasm_instance = new WebAssembly.Instance(wasm_module, wasm_import_objects);

            // Deal with exports"#
        );
        fn is_primary_type(val_type: &Type) -> bool {
            match val_type {
                Type::Bool | Type::Char |
                Type::Float32 | Type::Float64 |
                Type::S8 | Type::S16 | Type::S32 | Type::S64 |
                Type::U8 | Type::U16 | Type::U32 | Type::U64 => true,
                _ => false, 
            }
        }

        // If there is any function that accepts or returns a string
        // additional code need to be generated for JS string encoding and decoding
        let mut exist_string_as_param = false;
        let mut exist_string_as_result = false;
        for (_name, func) in funcs {
            for (_name, val_type) in &func.params {
                match val_type {
                    Type::String => {
                        exist_string_as_param = true;
                    },
                    _ => (),
                }
            }
            match &func.results {
                Results::Anon(val_type) =>
                    match val_type {
                        Type::String => {
                            exist_string_as_result = true;
                        },
                        _ => (),
                    },
                Results::Named(params) =>
                    for (_name, val_type) in params {
                        match val_type {
                            Type::String => {
                                exist_string_as_result = true;
                            },
                            _ => (),
                        }
                    },
            }
            if exist_string_as_param || exist_string_as_result {
                break;
            }
        }
        if exist_string_as_param || exist_string_as_result {
            uwriteln!(
                self.src,
                "const wasm_export_memory = wasm_instance.exports.memory;"
            );
        }
        if exist_string_as_param {
            uwriteln!(
                self.src,
                r#"const wasm_export_realloc = wasm_instance.exports.cabi_realloc;
                const wasm_wrapper_text_encoder = new TextEncoder();"#
            );
        }
        if exist_string_as_result {
            uwriteln!(
                self.src,
                "const wasm_wrapper_text_decoder = new TextDecoder();"
            );
        }
        if exist_string_as_param {
            uwriteln!(
                self.src,
                r#"
                // encode a string into UTF-8 and store it into the WASM linear memory
                function wasm_wrapper_encode_str(str) {{
                    if (typeof str !== "string") {{
                        throw new TypeError('expected a string');
                    }}
                    if (str.length == 0) {{
                        return {{ptr:1, len:0}};
                    }}
                    // encode the string into UTF-8
                    let encoded = wasm_wrapper_text_encoder.encode(str);
                    let len = encoded.length;
                    // allocate memory in the WASM linear memory for the string
                    let ptr = wasm_export_realloc(0, 0, 1, len);
                    // copy encoded string
                    let view = new Uint8Array(wasm_export_memory.buffer, ptr, len);
                    view.set(encoded);
                    return {{ptr, len}};
                }}"#
            );
        }
        if exist_string_as_result {
            uwriteln!(
                self.src,
                r#"
                function wasm_wrapper_decode_str(ptr, len) {{
                    let view = new Uint8Array(wasm_export_memory.buffer, ptr, len);
                    return wasm_wrapper_text_decoder.decode(view);
                }}
                "#
            );
        }


        for (func_name, func) in funcs {
            // wether this function only aceept primary types as arguments and return only primary types
            let mut is_primary_func = true;
            for (_name, val_type) in &func.params {
                if ! is_primary_type(val_type) {
                    is_primary_func = false;
                    break;
                }
            }
            if is_primary_func {
                match &func.results {
                    Results::Anon(val_type) =>
                        is_primary_func = is_primary_type(val_type),
                    Results::Named(params) =>
                        for (_name, val_type) in params {
                            if ! is_primary_type(val_type) {
                                is_primary_func = false;
                                break;
                            }
                        }
                }
            }

            if is_primary_func {
                uwriteln!(
                    self.src,
                    r#"let wasm_export_{} = wasm_instance.exports["{}"];"#,
                    func_name.to_lower_camel_case(), func_name
                );
            } else {
                uwrite!(self.src, "function wasm_export_{}(", func_name.to_lower_camel_case());
                for (param_name, _param_type) in &func.params {
                    uwrite!(self.src, "{}, ", param_name);
                }
                uwriteln!(self.src, ") {{");
                let mut arg_cnt = 0;
                for (param_name, param_type) in &func.params {
                    match param_type {
                        Type::String => {
                            uwriteln!(self.src, "let {}_encoded = wasm_wrapper_encode_str({});", param_name, param_name);
                            uwriteln!(self.src, "let arg{} = {}_encoded.ptr;", arg_cnt, param_name);
                            uwriteln!(self.src, "let arg{} = {}_encoded.len;", arg_cnt+1, param_name);
                            arg_cnt += 2;
                        },
                        Type::Id(_) => {
                            todo!("wrappaer for recursive types not implemented");
                        },
                        _ => {
                            uwriteln!(self.src, "let arg{} = {};", arg_cnt, param_name);
                            arg_cnt += 1;
                        }
                    }
                }

                uwriteln!(self.src, "");
                uwrite!(self.src, r#"let wasm_func_result = wasm_instance.exports["{}"]("#, func_name);
                for i in 0..arg_cnt {
                    uwrite!(self.src, "arg{}, ", i);
                }
                uwriteln!(self.src, ");");
                uwriteln!(self.src, "");
                // TODO: decode string
                // TODO: return result
                match func.results {
                    Results::Anon(result_type) => {
                        match result_type {
                            Type::Id(_) => {
                                todo!("multiple returning recursive types not implemented");
                            },
                            Type::String => {
                                uwriteln!(
                                    self.src,
                                    r#"// encode the string
                                    const wasm_func_result_ptr = new DataView(wasm_export_memory.buffer).getInt32(wasm_func_result, true);
                                    const wasm_func_result_len = new DataView(wasm_export_memory.buffer).getInt32(wasm_func_result+4, true);
                                    const js_func_result = wasm_wrapper_decode_str(wasm_func_result_ptr, wasm_func_result_len);
                                    "#
                                );
                            },
                            _ => ()
                        }
                    },
                    Results::Named(_) => {
                        todo!("multiple return values with recursive types not implemented");
                    },
                }

                uwriteln!(
                    self.src,
                    r#"let post_return = wasm_instance.exports["cabi_post_{}"];
                    if (post_return) {{
                        post_return(wasm_func_result);
                    }}

                    return js_func_result;"#,
                    func_name
                );
                uwriteln!(self.src, "}}\n");
            }
        }

        uwriteln!(self.src, "");
        uwrite!(self.src, "export {{");
        for (name, _func) in funcs {
            uwrite!(self.src, "wasm_export_{} as {}, ", name.to_lower_camel_case(), name.to_lower_camel_case());
        }
        uwriteln!(self.src, "}};");
    }

    fn export_types(
        &mut self,
        resolve: &Resolve,
        world: WorldId,
        types: &[(&str, TypeId)],
        _files: &mut Files,
    ) {
        println!("warning: export types are ignored when generating JS bindings.")
    }

    fn finish(&mut self, resolve: &Resolve, world: WorldId, files: &mut Files) {
        let world = &resolve.worlds[world];
        files.push(&format!("{}.js", world.name), self.src.as_bytes());
    }
}
