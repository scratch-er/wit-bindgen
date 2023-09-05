use heck::*;
use std::collections::HashMap;
use std::fmt::Write;
use wit_bindgen_core::{
    uwrite, uwriteln, wit_parser, Files, InterfaceGenerator as _, Source, WorldGenerator,
};
use wit_parser::*;

#[derive(Default)]
struct Js {
    src: Source,
    opts: Opts,
    hrefs: HashMap<String, String>,
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
        if self.opts.qjs {
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
                func_name, iface_name, func_name
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
                iface_name, func_name, iface_name, func_name
            );
        }
    }

    fn import_funcs(
        &mut self,
        _resolve: &Resolve,
        _world: WorldId,
        _funcs: &[(&str, &Function)],
        _files: &mut Files,
    ) {
        todo!("import_funcs() not implemented");
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
                    r#"let wasm_export_{} =  wasm_instance.exports["{}"];"#,
                    func_name, func_name
                );
            } else {
                uwrite!(self.src, "function wasm_export_{}(", func_name);
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
            uwrite!(self.src, "wasm_export_{} as {}, ", name, name);
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
        todo!("export_types() not implemented");
    }

    fn finish(&mut self, resolve: &Resolve, world: WorldId, files: &mut Files) {
        let world = &resolve.worlds[world];
        files.push(&format!("{}.js", world.name), self.src.as_bytes());
    }
}

impl Js {
    fn interface<'a>(&'a mut self, resolve: &'a Resolve) -> InterfaceGenerator<'_> {
        InterfaceGenerator {
            gen: self,
            resolve,
            types_header_printed: false,
        }
    }
}

struct InterfaceGenerator<'a> {
    gen: &'a mut Js,
    resolve: &'a Resolve,
    types_header_printed: bool,
}

impl InterfaceGenerator<'_> {
    fn funcs(&mut self, id: InterfaceId) {
        let iface = &self.resolve.interfaces[id];
        if iface.functions.is_empty() {
            return;
        }
        self.push_str("----\n\n");
        self.push_str("### Functions\n\n");
        for (_name, func) in iface.functions.iter() {
            self.func(func);
        }
    }

    fn func(&mut self, func: &Function) {
        self.push_str(&format!(
            "#### <a name=\"{0}\">`",
            func.name.to_snake_case()
        ));
        self.gen
            .hrefs
            .insert(func.name.clone(), format!("#{}", func.name.to_snake_case()));
        self.push_str(&func.name);
        self.push_str(": func`</a>");
        self.push_str("\n\n");
        self.docs(&func.docs);

        if func.params.len() > 0 {
            self.push_str("\n");
            self.push_str("##### Params\n\n");
            for (name, ty) in func.params.iter() {
                self.push_str(&format!(
                    "- <a name=\"{f}.{p}\">`{}`</a>: ",
                    name,
                    f = func.name.to_snake_case(),
                    p = name.to_snake_case(),
                ));
                self.print_ty(ty);
                self.push_str("\n");
            }
        }

        if func.results.len() > 0 {
            self.push_str("\n##### Return values\n\n");
            match &func.results {
                Results::Named(params) => {
                    for (name, ty) in params.iter() {
                        self.push_str(&format!(
                            "- <a name=\"{f}.{p}\">`{}`</a>: ",
                            name,
                            f = func.name.to_snake_case(),
                            p = name,
                        ));
                        self.print_ty(ty);
                        self.push_str("\n");
                    }
                }
                Results::Anon(ty) => {
                    self.push_str(&format!(
                        "- <a name=\"{f}.0\"></a> ",
                        f = func.name.to_snake_case(),
                    ));
                    self.print_ty(ty);
                    self.push_str("\n");
                }
            }
        }

        self.push_str("\n");
    }

    fn push_str(&mut self, s: &str) {
        self.gen.src.push_str(s);
    }

    fn print_ty(&mut self, ty: &Type) {
        match ty {
            Type::Bool => self.push_str("`bool`"),
            Type::U8 => self.push_str("`u8`"),
            Type::S8 => self.push_str("`s8`"),
            Type::U16 => self.push_str("`u16`"),
            Type::S16 => self.push_str("`s16`"),
            Type::U32 => self.push_str("`u32`"),
            Type::S32 => self.push_str("`s32`"),
            Type::U64 => self.push_str("`u64`"),
            Type::S64 => self.push_str("`s64`"),
            Type::Float32 => self.push_str("`float32`"),
            Type::Float64 => self.push_str("`float64`"),
            Type::Char => self.push_str("`char`"),
            Type::String => self.push_str("`string`"),
            Type::Id(id) => {
                let ty = &self.resolve.types[*id];
                if let Some(name) = &ty.name {
                    self.push_str("[`");
                    self.push_str(name);
                    self.push_str("`](#");
                    self.push_str(&name.to_snake_case());
                    self.push_str(")");
                    return;
                }
                match &ty.kind {
                    TypeDefKind::Type(t) => self.print_ty(t),
                    TypeDefKind::Tuple(t) => {
                        self.push_str("(");
                        for (i, t) in t.types.iter().enumerate() {
                            if i > 0 {
                                self.push_str(", ");
                            }
                            self.print_ty(t);
                        }
                        self.push_str(")");
                    }
                    TypeDefKind::Record(_)
                    | TypeDefKind::Flags(_)
                    | TypeDefKind::Enum(_)
                    | TypeDefKind::Variant(_)
                    | TypeDefKind::Union(_) => {
                        // These types are always named, so we will have
                        // printed the name above, so we don't need to print
                        // the contents.
                        assert!(ty.name.is_some());
                    }
                    TypeDefKind::Option(t) => {
                        self.push_str("option<");
                        self.print_ty(t);
                        self.push_str(">");
                    }
                    TypeDefKind::Result(r) => match (r.ok, r.err) {
                        (Some(ok), Some(err)) => {
                            self.push_str("result<");
                            self.print_ty(&ok);
                            self.push_str(", ");
                            self.print_ty(&err);
                            self.push_str(">");
                        }
                        (None, Some(err)) => {
                            self.push_str("result<_, ");
                            self.print_ty(&err);
                            self.push_str(">");
                        }
                        (Some(ok), None) => {
                            self.push_str("result<");
                            self.print_ty(&ok);
                            self.push_str(">");
                        }
                        (None, None) => {
                            self.push_str("result");
                        }
                    },
                    TypeDefKind::List(t) => {
                        self.push_str("list<");
                        self.print_ty(t);
                        self.push_str(">");
                    }
                    TypeDefKind::Future(t) => match t {
                        Some(t) => {
                            self.push_str("future<");
                            self.print_ty(t);
                            self.push_str(">");
                        }
                        None => {
                            self.push_str("future");
                        }
                    },
                    TypeDefKind::Stream(s) => match (s.element, s.end) {
                        (Some(element), Some(end)) => {
                            self.push_str("stream<");
                            self.print_ty(&element);
                            self.push_str(", ");
                            self.print_ty(&end);
                            self.push_str(">");
                        }
                        (None, Some(end)) => {
                            self.push_str("stream<_, ");
                            self.print_ty(&end);
                            self.push_str(">");
                        }
                        (Some(element), None) => {
                            self.push_str("stream<");
                            self.print_ty(&element);
                            self.push_str(">");
                        }
                        (None, None) => {
                            self.push_str("stream");
                        }
                    },
                    TypeDefKind::Resource | TypeDefKind::Handle(_) => {
                        todo!("implement resources")
                    }
                    TypeDefKind::Unknown => unreachable!(),
                }
            }
        }
    }

    fn docs(&mut self, docs: &Docs) {
        let docs = match &docs.contents {
            Some(docs) => docs,
            None => return,
        };
        for line in docs.lines() {
            self.push_str(line.trim());
            self.push_str("\n");
        }
    }

    fn print_type_header(&mut self, type_: &str, name: &str) {
        if !self.types_header_printed {
            self.push_str("----\n\n");
            self.push_str("### Types\n\n");
            self.types_header_printed = true;
        }
        self.push_str(&format!(
            "#### <a name=\"{}\">`{} {}`</a>\n",
            name.to_snake_case(),
            type_,
            name,
        ));
        self.gen
            .hrefs
            .insert(name.to_string(), format!("#{}", name.to_snake_case()));
    }
}

impl<'a> wit_bindgen_core::InterfaceGenerator<'a> for InterfaceGenerator<'a> {
    fn resolve(&self) -> &'a Resolve {
        self.resolve
    }

    fn type_record(&mut self, _id: TypeId, name: &str, record: &Record, docs: &Docs) {
        self.print_type_header("record", name);
        self.push_str("\n");
        self.docs(docs);
        self.push_str("\n##### Record Fields\n\n");
        for field in record.fields.iter() {
            self.push_str(&format!(
                "- <a name=\"{r}.{f}\">`{name}`</a>: ",
                r = name.to_snake_case(),
                f = field.name.to_snake_case(),
                name = field.name,
            ));
            self.gen.hrefs.insert(
                format!("{}::{}", name, field.name),
                format!("#{}.{}", name.to_snake_case(), field.name.to_snake_case()),
            );
            self.print_ty(&field.ty);
            if field.docs.contents.is_some() {
                self.gen.src.indent(1);
                self.push_str("\n<p>");
                self.docs(&field.docs);
                self.gen.src.deindent(1);
            }
            self.push_str("\n");
        }
    }

    fn type_tuple(&mut self, _id: TypeId, name: &str, tuple: &Tuple, docs: &Docs) {
        self.print_type_header("tuple", name);
        self.push_str("\n");
        self.docs(docs);
        self.push_str("\n##### Tuple Fields\n\n");
        for (i, ty) in tuple.types.iter().enumerate() {
            self.push_str(&format!(
                "- <a name=\"{r}.{f}\">`{name}`</a>: ",
                r = name.to_snake_case(),
                f = i,
                name = i,
            ));
            self.gen.hrefs.insert(
                format!("{}::{}", name, i),
                format!("#{}.{}", name.to_snake_case(), i),
            );
            self.print_ty(ty);
            self.push_str("\n");
        }
    }

    fn type_flags(&mut self, _id: TypeId, name: &str, flags: &Flags, docs: &Docs) {
        self.print_type_header("flags", name);
        self.push_str("\n");
        self.docs(docs);
        self.push_str("\n##### Flags members\n\n");
        for flag in flags.flags.iter() {
            self.push_str(&format!(
                "- <a name=\"{r}.{f}\">`{name}`</a>: ",
                r = name.to_snake_case(),
                f = flag.name.to_snake_case(),
                name = flag.name,
            ));
            self.gen.hrefs.insert(
                format!("{}::{}", name, flag.name),
                format!("#{}.{}", name.to_snake_case(), flag.name.to_snake_case()),
            );
            if flag.docs.contents.is_some() {
                self.gen.src.indent(1);
                self.push_str("\n<p>");
                self.docs(&flag.docs);
                self.gen.src.deindent(1);
            }
            self.push_str("\n");
        }
    }

    fn type_variant(&mut self, _id: TypeId, name: &str, variant: &Variant, docs: &Docs) {
        self.print_type_header("variant", name);
        self.push_str("\n");
        self.docs(docs);
        self.push_str("\n##### Variant Cases\n\n");
        for case in variant.cases.iter() {
            self.push_str(&format!(
                "- <a name=\"{v}.{c}\">`{name}`</a>",
                v = name.to_snake_case(),
                c = case.name.to_snake_case(),
                name = case.name,
            ));
            self.gen.hrefs.insert(
                format!("{}::{}", name, case.name),
                format!("#{}.{}", name.to_snake_case(), case.name.to_snake_case()),
            );
            if let Some(ty) = &case.ty {
                self.push_str(": ");
                self.print_ty(ty);
            }
            if case.docs.contents.is_some() {
                self.gen.src.indent(1);
                self.push_str("\n<p>");
                self.docs(&case.docs);
                self.gen.src.deindent(1);
            }
            self.push_str("\n");
        }
    }

    fn type_union(&mut self, _id: TypeId, name: &str, union: &Union, docs: &Docs) {
        self.print_type_header("union", name);
        self.push_str("\n");
        self.docs(docs);
        self.push_str("\n##### Union Cases\n\n");
        let snake = name.to_snake_case();
        for (i, case) in union.cases.iter().enumerate() {
            self.push_str(&format!("- <a name=\"{snake}.{i}\">`{i}`</a>",));
            self.gen
                .hrefs
                .insert(format!("{name}::{i}"), format!("#{snake}.{i}"));
            self.push_str(": ");
            self.print_ty(&case.ty);
            if case.docs.contents.is_some() {
                self.gen.src.indent(1);
                self.push_str("\n<p>");
                self.docs(&case.docs);
                self.gen.src.deindent(1);
            }
            self.push_str("\n");
        }
    }

    fn type_enum(&mut self, _id: TypeId, name: &str, enum_: &Enum, docs: &Docs) {
        self.print_type_header("enum", name);
        self.push_str("\n");
        self.docs(docs);
        self.push_str("\n##### Enum Cases\n\n");
        for case in enum_.cases.iter() {
            self.push_str(&format!(
                "- <a name=\"{v}.{c}\">`{name}`</a>",
                v = name.to_snake_case(),
                c = case.name.to_snake_case(),
                name = case.name,
            ));
            self.gen.hrefs.insert(
                format!("{}::{}", name, case.name),
                format!("#{}.{}", name.to_snake_case(), case.name.to_snake_case()),
            );
            if case.docs.contents.is_some() {
                self.gen.src.indent(1);
                self.push_str("\n<p>");
                self.docs(&case.docs);
                self.gen.src.deindent(1);
            }
            self.push_str("\n");
        }
    }

    fn type_option(&mut self, _id: TypeId, name: &str, payload: &Type, docs: &Docs) {
        self.print_type_header("type", name);
        self.push_str("option<");
        self.print_ty(payload);
        self.push_str(">");
        self.push_str("\n");
        self.docs(docs);
    }

    fn type_result(&mut self, _id: TypeId, name: &str, result: &Result_, docs: &Docs) {
        self.print_type_header("type", name);
        match (result.ok, result.err) {
            (Some(ok), Some(err)) => {
                self.push_str("result<");
                self.print_ty(&ok);
                self.push_str(", ");
                self.print_ty(&err);
                self.push_str(">");
            }
            (None, Some(err)) => {
                self.push_str("result<_, ");
                self.print_ty(&err);
                self.push_str(">");
            }
            (Some(ok), None) => {
                self.push_str("result<");
                self.print_ty(&ok);
                self.push_str(">");
            }
            (None, None) => {
                self.push_str("result");
            }
        }
        self.push_str("\n");
        self.docs(docs);
    }

    fn type_alias(&mut self, _id: TypeId, name: &str, ty: &Type, docs: &Docs) {
        self.print_type_header("type", name);
        self.print_ty(ty);
        self.push_str("\n<p>");
        self.docs(docs);
        self.push_str("\n");
    }

    fn type_list(&mut self, id: TypeId, name: &str, _ty: &Type, docs: &Docs) {
        self.type_alias(id, name, &Type::Id(id), docs);
    }

    fn type_builtin(&mut self, id: TypeId, name: &str, ty: &Type, docs: &Docs) {
        self.type_alias(id, name, ty, docs)
    }
}
