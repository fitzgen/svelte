use super::Parse;
use std::collections::HashMap;
use twiggy_ir::{self as ir, Id};
use twiggy_traits as traits;
use wasmparser::SectionWithLimitedItems;
use wasmparser::{self, Operator, SectionReader, Type};

#[derive(Default)]
pub struct SectionIndices {
    type_: Option<usize>,
    code: Option<usize>,
    functions: Vec<Id>,
    tables: Vec<Id>,
    memories: Vec<Id>,
    globals: Vec<Id>,
}

struct IndexedSection<'a>(usize, wasmparser::Section<'a>);

impl<'a> Parse<'a> for wasmparser::ModuleReader<'a> {
    type ItemsExtra = ();

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        _extra: (),
    ) -> Result<(), traits::Error> {
        let initial_offset = self.current_position();
        let mut sections: Vec<IndexedSection<'_>> = Vec::new();
        let mut code_section: Option<IndexedSection<'_>> = None;
        let mut function_section: Option<IndexedSection<'_>> = None;
        let mut sizes: HashMap<usize, u32> = HashMap::new();

        // The function and code sections must be handled differently, so these
        // are not placed in the same `sections` array as the rest.
        let mut idx = 0;
        while !self.eof() {
            let start = self.current_position();
            let section = self.read()?;
            let size = self.current_position() - start;
            let indexed_section = IndexedSection(idx, section);
            match indexed_section.1.code {
                wasmparser::SectionCode::Code => code_section = Some(indexed_section),
                wasmparser::SectionCode::Function => function_section = Some(indexed_section),
                _ => sections.push(indexed_section),
            };
            sizes.insert(idx, size as u32);
            idx += 1;
        }

        let sections_cnt = sections.len()
            + if code_section.is_some() { 1 } else { 0 }
            + if function_section.is_some() { 1 } else { 0 };
        let id = Id::section(sections_cnt);
        items.add_root(ir::Item::new(
            id,
            "wasm magic bytes".to_string(),
            initial_offset as u32,
            ir::Misc::new(),
        ));

        // Before we actually parse any items prepare to parse a few sections
        // below, namely the code section. When parsing the code section we want
        // to try to assign human-readable names so we need the name section, if
        // present. Additionally we need to look at the number of imported
        // functions to handle the wasm function index space correctly.
        let names = parse_names_section(&sections)?;
        let imported_functions = count_imported_functions(&sections)?;

        // Next, we parse the function and code sections together, so that we
        // can collapse corresponding entries from the code and function
        // sections into a single representative IR item.
        match (function_section, code_section) {
            (Some(function_section), Some(code_section)) => (function_section, code_section)
                .parse_items(items, (imported_functions, &names, &sizes))?,
            _ => Err(traits::Error::with_msg(
                "function or code section is missing",
            ))?,
        };

        for IndexedSection(idx, section) in sections.into_iter() {
            let start = items.size_added();
            let name = get_section_name(&section);
            match section.code {
                wasmparser::SectionCode::Custom { name, .. } => {
                    CustomSectionReader(name, section).parse_items(items, idx)?;
                }
                wasmparser::SectionCode::Type => {
                    section.get_type_section_reader()?.parse_items(items, idx)?;
                }
                wasmparser::SectionCode::Import => {
                    section
                        .get_import_section_reader()?
                        .parse_items(items, idx)?;
                }
                wasmparser::SectionCode::Table => {
                    section
                        .get_table_section_reader()?
                        .parse_items(items, idx)?;
                }
                wasmparser::SectionCode::Memory => {
                    section
                        .get_memory_section_reader()?
                        .parse_items(items, idx)?;
                }
                wasmparser::SectionCode::Global => {
                    section
                        .get_global_section_reader()?
                        .parse_items(items, idx)?;
                }
                wasmparser::SectionCode::Export => {
                    section
                        .get_export_section_reader()?
                        .parse_items(items, idx)?;
                }
                wasmparser::SectionCode::Start => {
                    StartSection(section).parse_items(items, idx)?;
                }
                wasmparser::SectionCode::Element => {
                    section
                        .get_element_section_reader()?
                        .parse_items(items, idx)?;
                }
                wasmparser::SectionCode::Data => {
                    section.get_data_section_reader()?.parse_items(items, idx)?;
                }
                wasmparser::SectionCode::DataCount => {
                    let msg = "the last iteration of the loop take 7 as id";
                    dbg!(msg, idx);
                    DataCountSection(section).parse_items(items, idx)?;
                }
                wasmparser::SectionCode::Code | wasmparser::SectionCode::Function => {
                    unreachable!("unexpected code or function section found");
                }
            };
            if idx == 7 {
                let msg = "At some point just after the debug above this also is 7 so the panic";
                dbg!(msg, &idx);
            }
            let id = Id::section(idx);
            let added = items.size_added() - start;
            let size = sizes
                .get(&idx)
                .ok_or_else(|| traits::Error::with_msg("Could not find section size"))?;
            assert!(added <= *size);
            items.add_root(ir::Item::new(id, name, size - added, ir::Misc::new()));
        }

        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(
        &mut self,
        items: &mut ir::ItemsBuilder,
        _extra: (),
    ) -> Result<(), traits::Error> {
        let mut sections: Vec<IndexedSection<'_>> = Vec::new();
        let mut code_section: Option<IndexedSection<'a>> = None;
        let mut function_section: Option<IndexedSection<'a>> = None;

        let mut idx = 0;
        while !self.eof() {
            let section = self.read()?;
            match section.code {
                wasmparser::SectionCode::Code => code_section = Some(IndexedSection(idx, section)),
                wasmparser::SectionCode::Function => {
                    function_section = Some(IndexedSection(idx, section))
                }
                _ => sections.push(IndexedSection(idx, section)),
            };
            idx += 1;
        }

        // Like above we do some preprocessing here before actually drawing all
        // the edges below. Here we primarily want to learn some properties of
        // the wasm module, such as what `Id` is mapped to all index spaces in
        // the wasm module. To handle that we build up all this data in
        // `SectionIndices` here as we parse all the various sections.
        let mut indices = SectionIndices::default();
        for IndexedSection(idx, section) in sections.iter() {
            match section.code {
                wasmparser::SectionCode::Type => {
                    indices.type_ = Some(*idx);
                }
                wasmparser::SectionCode::Import => {
                    let reader = section.get_import_section_reader()?;
                    for (i, import) in reader.into_iter().enumerate() {
                        let id = Id::entry(*idx, i);
                        match import?.ty {
                            wasmparser::ImportSectionEntryType::Function(_) => {
                                indices.functions.push(id);
                            }
                            wasmparser::ImportSectionEntryType::Table(_) => {
                                indices.tables.push(id);
                            }
                            wasmparser::ImportSectionEntryType::Memory(_) => {
                                indices.memories.push(id);
                            }
                            wasmparser::ImportSectionEntryType::Global(_) => {
                                indices.globals.push(id);
                            }
                        }
                    }
                }
                wasmparser::SectionCode::Global => {
                    for i in 0..section.get_global_section_reader()?.get_count() {
                        let id = Id::entry(*idx, i as usize);
                        indices.globals.push(id);
                    }
                }
                wasmparser::SectionCode::Memory => {
                    for i in 0..section.get_memory_section_reader()?.get_count() {
                        let id = Id::entry(*idx, i as usize);
                        indices.memories.push(id);
                    }
                }
                wasmparser::SectionCode::Table => {
                    for i in 0..section.get_table_section_reader()?.get_count() {
                        let id = Id::entry(*idx, i as usize);
                        indices.tables.push(id);
                    }
                }
                wasmparser::SectionCode::Code => {
                    Err(traits::Error::with_msg("unexpected code section"))?
                }
                wasmparser::SectionCode::Function => {
                    Err(traits::Error::with_msg("unexpected function section"))?
                }
                _ => {}
            }
        }
        if let (Some(IndexedSection(_, function_section)), Some(IndexedSection(code_idx, _))) =
            (function_section.as_ref(), code_section.as_ref())
        {
            indices.code = Some(*code_idx);
            for i in 0..function_section.get_function_section_reader()?.get_count() {
                let id = Id::entry(*code_idx, i as usize);
                indices.functions.push(id);
            }
        }

        match (function_section, code_section) {
            (Some(function_section), Some(code_section)) => {
                (function_section, code_section).parse_edges(items, &indices)?
            }
            _ => panic!("function or code section is missing"),
        };
        for IndexedSection(idx, section) in sections.into_iter() {
            match section.code {
                wasmparser::SectionCode::Custom { name, .. } => {
                    CustomSectionReader(name, section).parse_edges(items, ())?;
                }
                wasmparser::SectionCode::Type => {
                    section.get_type_section_reader()?.parse_edges(items, ())?;
                }
                wasmparser::SectionCode::Import => {
                    section
                        .get_import_section_reader()?
                        .parse_edges(items, ())?;
                }
                wasmparser::SectionCode::Table => {
                    section.get_table_section_reader()?.parse_edges(items, ())?;
                }
                wasmparser::SectionCode::Memory => {
                    section
                        .get_memory_section_reader()?
                        .parse_edges(items, ())?;
                }
                wasmparser::SectionCode::Global => {
                    section
                        .get_global_section_reader()?
                        .parse_edges(items, ())?;
                }
                wasmparser::SectionCode::Export => {
                    section
                        .get_export_section_reader()?
                        .parse_edges(items, (&indices, idx))?;
                }
                wasmparser::SectionCode::Start => {
                    StartSection(section).parse_edges(items, (&indices, idx))?;
                }
                wasmparser::SectionCode::Element => {
                    section
                        .get_element_section_reader()?
                        .parse_edges(items, (&indices, idx))?;
                }
                wasmparser::SectionCode::Data => {
                    section.get_data_section_reader()?.parse_edges(items, ())?;
                }
                wasmparser::SectionCode::DataCount => {
                    DataCountSection(section).parse_edges(items, ())?;
                }
                wasmparser::SectionCode::Code | wasmparser::SectionCode::Function => {
                    unreachable!("unexpected code or function section found");
                }
            }
        }

        Ok(())
    }
}

fn get_section_name(section: &wasmparser::Section<'_>) -> String {
    match section.code {
        wasmparser::SectionCode::Custom { name, .. } => {
            format!("custom section '{}' headers", name)
        }
        wasmparser::SectionCode::Type => "type section headers".to_string(),
        wasmparser::SectionCode::Import => "import section headers".to_string(),
        wasmparser::SectionCode::Function => "function section headers".to_string(),
        wasmparser::SectionCode::Table => "table section headers".to_string(),
        wasmparser::SectionCode::Memory => "memory section headers".to_string(),
        wasmparser::SectionCode::Global => "global section headers".to_string(),
        wasmparser::SectionCode::Export => "export section headers".to_string(),
        wasmparser::SectionCode::Start => "start section headers".to_string(),
        wasmparser::SectionCode::Element => "element section headers".to_string(),
        wasmparser::SectionCode::Code => "code section headers".to_string(),
        wasmparser::SectionCode::Data => "data section headers".to_string(),
        wasmparser::SectionCode::DataCount => "data count section headers".to_string(),
    }
}

fn parse_names_section<'a>(
    indexed_sections: &[IndexedSection<'a>],
) -> Result<HashMap<usize, &'a str>, traits::Error> {
    let mut names = HashMap::new();
    for IndexedSection(_, section) in indexed_sections.iter() {
        if let wasmparser::SectionCode::Custom { name: "name", .. } = section.code {
            for subsection in section.get_name_section_reader()? {
                let f = match subsection? {
                    wasmparser::Name::Function(f) => f,
                    _ => continue,
                };
                let mut map = f.get_map()?;
                for _ in 0..map.get_count() {
                    let naming = map.read()?;
                    names.insert(naming.index as usize, naming.name);
                }
            }
        }
    }
    Ok(names)
}

fn count_imported_functions<'a>(
    indexed_sections: &[IndexedSection<'a>],
) -> Result<usize, traits::Error> {
    let mut imported_functions = 0;
    for IndexedSection(_, section) in indexed_sections.iter() {
        if let wasmparser::SectionCode::Import = section.code {
            for import in section.get_import_section_reader()? {
                if let wasmparser::ImportSectionEntryType::Function(_) = import?.ty {
                    imported_functions += 1;
                }
            }
        }
    }
    Ok(imported_functions)
}

impl<'a> Parse<'a> for (IndexedSection<'a>, IndexedSection<'a>) {
    type ItemsExtra = (usize, &'a HashMap<usize, &'a str>, &'a HashMap<usize, u32>);

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        (imported_functions, names, sizes): Self::ItemsExtra,
    ) -> Result<(), traits::Error> {
        let (
            IndexedSection(func_section_idx, func_section),
            IndexedSection(code_section_idx, code_section),
        ) = self;

        let mut func_reader = func_section.get_function_section_reader()?;
        let mut code_reader = code_section.get_code_section_reader()?;

        let func_items: Vec<ir::Item> = iterate_with_size(&mut func_reader)
            .enumerate()
            .map(|(i, func)| {
                let (_func, size) = func?;
                let id = Id::entry(*func_section_idx, i);
                let name = format!("func[{}]", i);
                let item = ir::Item::new(id, name, size, ir::Misc::new());
                Ok(item)
            })
            .collect::<Result<_, traits::Error>>()?;

        let code_items: Vec<ir::Item> = iterate_with_size(&mut code_reader)
            .zip(func_items.into_iter())
            .enumerate()
            .map(|(i, (body, func))| {
                let (_body, size) = body?;
                let id = Id::entry(*code_section_idx, i);
                let name = names
                    .get(&(i + imported_functions))
                    .map_or_else(|| format!("code[{}]", i), |name| name.to_string());
                let code = ir::Code::new(&name);
                let item = ir::Item::new(id, name, size + func.size(), code);
                Ok(item)
            })
            .collect::<Result<_, traits::Error>>()?;

        let start = items.size_added();
        let name = get_section_name(code_section);
        for item in code_items.into_iter() {
            items.add_item(item);
        }
        let id = Id::section(*code_section_idx);
        let added = items.size_added() - start;
        let size = sizes
            .get(&code_section_idx)
            .ok_or_else(|| traits::Error::with_msg("Could not find section size"))?
            + sizes
                .get(&func_section_idx)
                .ok_or_else(|| traits::Error::with_msg("Could not find section size"))?;
        assert!(added <= size);
        items.add_root(ir::Item::new(id, name, size - added, ir::Misc::new()));

        Ok(())
    }

    type EdgesExtra = &'a SectionIndices;

    fn parse_edges(
        &mut self,
        items: &mut ir::ItemsBuilder,
        indices: Self::EdgesExtra,
    ) -> Result<(), traits::Error> {
        let (IndexedSection(_, function_section), IndexedSection(code_section_idx, code_section)) =
            self;

        let mut func_reader = function_section.get_function_section_reader()?;
        let mut code_reader = code_section.get_code_section_reader()?;

        type Edge = (ir::Id, ir::Id);

        let mut edges: Vec<Edge> = Vec::new();

        // Function section reader parsing.
        for (func_i, type_ref) in iterate_with_size(&mut func_reader).enumerate() {
            let (type_ref, _) = type_ref?;
            if let Some(type_idx) = indices.type_ {
                let type_id = Id::entry(type_idx, type_ref as usize);
                if let Some(code_idx) = indices.code {
                    let body_id = Id::entry(code_idx, func_i);
                    edges.push((body_id, type_id));
                }
            }
        }

        // Code section reader parsing.
        for (b_i, body) in iterate_with_size(&mut code_reader).enumerate() {
            let (body, _size) = body?;
            let body_id = Id::entry(*code_section_idx, b_i);

            let mut cache = None;
            for op in body.get_operators_reader()? {
                let prev = cache.take();
                match op? {
                    Operator::Call { function_index } => {
                        let f_id = indices.functions[function_index as usize];
                        edges.push((body_id, f_id));
                    }

                    // TODO: Rather than looking at indirect calls, need to look
                    // at where the vtables get initialized and/or vtable
                    // indices get pushed onto the stack.
                    Operator::CallIndirect { .. } => continue,

                    Operator::GetGlobal { global_index } | Operator::SetGlobal { global_index } => {
                        let g_id = indices.globals[global_index as usize];
                        edges.push((body_id, g_id));
                    }

                    Operator::I32Load { memarg }
                    | Operator::I32Load8S { memarg }
                    | Operator::I32Load8U { memarg }
                    | Operator::I32Load16S { memarg }
                    | Operator::I32Load16U { memarg }
                    | Operator::I64Load { memarg }
                    | Operator::I64Load8S { memarg }
                    | Operator::I64Load8U { memarg }
                    | Operator::I64Load16S { memarg }
                    | Operator::I64Load16U { memarg }
                    | Operator::I64Load32S { memarg }
                    | Operator::I64Load32U { memarg }
                    | Operator::F32Load { memarg }
                    | Operator::F64Load { memarg } => {
                        if let Some(Operator::I32Const { value }) = prev {
                            if let Some(data_id) = items.get_data(value as u32 + memarg.offset) {
                                edges.push((body_id, data_id));
                            }
                        }
                    }
                    other => cache = Some(other),
                }
            }
        }

        edges
            .into_iter()
            .for_each(|(from, to)| items.add_edge(from, to));

        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::NameSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        let mut i = 0;
        while !self.eof() {
            let start = self.original_position();
            let subsection = self.read()?;
            let size = (self.original_position() - start) as u32;
            let name = match subsection {
                wasmparser::Name::Module(_) => "\"module name\" subsection",
                wasmparser::Name::Function(_) => "\"function names\" subsection",
                wasmparser::Name::Local(_) => "\"local names\" subsection",
            };
            let id = Id::entry(idx, i);
            items.add_root(ir::Item::new(id, name, size, ir::DebugInfo::new()));
            i += 1;
        }

        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

struct CustomSectionReader<'a>(&'a str, wasmparser::Section<'a>);

impl<'a> Parse<'a> for CustomSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        let name = self.0;
        if name == "name" {
            self.1.get_name_section_reader()?.parse_items(items, idx)?;
        } else {
            let range = self.1.get_binary_reader().range();
            let size = (range.end - range.start) as u32;
            let id = Id::entry(idx, 0);
            let name = format!("custom section '{}'", self.0);
            items.add_item(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::TypeSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, ty) in iterate_with_size(self).enumerate() {
            let (ty, size) = ty?;
            let id = Id::entry(idx, i);

            let mut name = format!("type[{}]: (", i);
            for (i, param) in ty.params.iter().enumerate() {
                if i != 0 {
                    name.push_str(", ");
                }
                name.push_str(ty2str(*param));
            }
            name.push_str(") -> ");

            match ty.returns.len() {
                0 => name.push_str("nil"),
                1 => name.push_str(ty2str(ty.returns[0])),
                _ => {
                    name.push_str("(");
                    for (i, result) in ty.returns.iter().enumerate() {
                        if i != 0 {
                            name.push_str(", ");
                        }
                        name.push_str(ty2str(*result));
                    }
                    name.push_str(")");
                }
            }

            items.add_item(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::ImportSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, imp) in iterate_with_size(self).enumerate() {
            let (imp, size) = imp?;
            let id = Id::entry(idx, i);
            let name = format!("import {}::{}", imp.module, imp.field);
            items.add_item(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, (): ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::TableSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, entry) in iterate_with_size(self).enumerate() {
            let (_entry, size) = entry?;
            let id = Id::entry(idx, i);
            let name = format!("table[{}]", i);
            items.add_root(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::MemorySectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, mem) in iterate_with_size(self).enumerate() {
            let (_mem, size) = mem?;
            let id = Id::entry(idx, i);
            let name = format!("memory[{}]", i);
            items.add_item(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::GlobalSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, g) in iterate_with_size(self).enumerate() {
            let (g, size) = g?;
            let id = Id::entry(idx, i);
            let name = format!("global[{}]", i);
            let ty = ty2str(g.ty.content_type).to_string();
            items.add_item(ir::Item::new(id, name, size, ir::Data::new(Some(ty))));
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::ExportSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, exp) in iterate_with_size(self).enumerate() {
            let (exp, size) = exp?;
            let id = Id::entry(idx, i);
            let name = format!("export \"{}\"", exp.field);
            items.add_root(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = (&'a SectionIndices, usize);

    fn parse_edges(
        &mut self,
        items: &mut ir::ItemsBuilder,
        (indices, idx): Self::EdgesExtra,
    ) -> Result<(), traits::Error> {
        for (i, exp) in iterate_with_size(self).enumerate() {
            let (exp, _) = exp?;
            let exp_id = Id::entry(idx, i);
            match exp.kind {
                wasmparser::ExternalKind::Function => {
                    items.add_edge(exp_id, indices.functions[exp.index as usize]);
                }
                wasmparser::ExternalKind::Table => {
                    items.add_edge(exp_id, indices.tables[exp.index as usize]);
                }
                wasmparser::ExternalKind::Memory => {
                    items.add_edge(exp_id, indices.memories[exp.index as usize]);
                }
                wasmparser::ExternalKind::Global => {
                    items.add_edge(exp_id, indices.globals[exp.index as usize]);
                }
            }
        }

        Ok(())
    }
}

struct StartSection<'a>(wasmparser::Section<'a>);

impl<'a> Parse<'a> for StartSection<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        let range = self.0.range();
        let size = (range.end - range.start) as u32;
        let id = Id::section(idx);
        let name = "\"start\" section";
        items.add_root(ir::Item::new(id, name, size, ir::Misc::new()));
        Ok(())
    }

    type EdgesExtra = (&'a SectionIndices, usize);

    fn parse_edges(
        &mut self,
        items: &mut ir::ItemsBuilder,
        (indices, idx): Self::EdgesExtra,
    ) -> Result<(), traits::Error> {
        let f_i = self.0.get_start_section_content()?;
        items.add_edge(Id::section(idx), indices.functions[f_i as usize]);
        Ok(())
    }
}

struct DataCountSection<'a>(wasmparser::Section<'a>);

impl<'a> Parse<'a> for DataCountSection<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        let range = self.0.range();
        let size = (range.end - range.start) as u32;
        //dbg!("section", &idx);
        let id = Id::section(idx);
        let name = "\"data count\" section";
        //dbg!("YEAH", &id);
        items.add_root(ir::Item::new(id, name, size, ir::Misc::new()));
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _items: &mut ir::ItemsBuilder, (): ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::ElementSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, elem) in iterate_with_size(self).enumerate() {
            let (_elem, size) = elem?;
            let id = Id::entry(idx, i);
            let name = format!("elem[{}]", i);
            items.add_item(ir::Item::new(id, name, size, ir::Misc::new()));
        }
        Ok(())
    }

    type EdgesExtra = (&'a SectionIndices, usize);

    fn parse_edges(
        &mut self,
        items: &mut ir::ItemsBuilder,
        (indices, idx): Self::EdgesExtra,
    ) -> Result<(), traits::Error> {
        for (i, elem) in iterate_with_size(self).enumerate() {
            let (elem, _size) = elem?;
            let elem_id = Id::entry(idx, i);

            match elem.kind {
                wasmparser::ElementKind::Active { table_index, .. } => {
                    items.add_edge(indices.tables[table_index as usize], elem_id);
                }
                wasmparser::ElementKind::Passive(_ty) => {}
            }
            for func_idx in elem.items.get_items_reader()? {
                let func_idx = func_idx?;
                items.add_edge(elem_id, indices.functions[func_idx as usize]);
            }
        }

        Ok(())
    }
}

impl<'a> Parse<'a> for wasmparser::DataSectionReader<'a> {
    type ItemsExtra = usize;

    fn parse_items(
        &mut self,
        items: &mut ir::ItemsBuilder,
        idx: usize,
    ) -> Result<(), traits::Error> {
        for (i, d) in iterate_with_size(self).enumerate() {
            let (d, size) = d?;
            let id = Id::entry(idx, i);
            let name = format!("data[{}]", i);
            items.add_item(ir::Item::new(id, name, size, ir::Data::new(None)));

            // Get the constant address (if any) from the initialization
            // expression.
            if let wasmparser::DataKind::Active { init_expr, .. } = d.kind {
                let mut iter = init_expr.get_operators_reader();
                let offset = match iter.read()? {
                    Operator::I32Const { value } => Some(i64::from(value)),
                    Operator::I64Const { value } => Some(value),
                    _ => None,
                };

                if let Some(off) = offset {
                    let length = d.data.len(); // size of data
                    items.link_data(off, length, id);
                }
            }
        }
        Ok(())
    }

    type EdgesExtra = ();

    fn parse_edges(&mut self, _: &mut ir::ItemsBuilder, _: ()) -> Result<(), traits::Error> {
        Ok(())
    }
}

fn iterate_with_size<'a, S: SectionWithLimitedItems + SectionReader>(
    s: &'a mut S,
) -> impl Iterator<Item = Result<(S::Item, u32), traits::Error>> + 'a {
    let count = s.get_count();
    (0..count).map(move |i| {
        let start = s.original_position();
        let item = s.read()?;
        let size = (s.original_position() - start) as u32;
        if i == count - 1 {
            s.ensure_end()?;
        }
        Ok((item, size))
    })
}

fn ty2str(t: Type) -> &'static str {
    match t {
        Type::I32 => "i32",
        Type::I64 => "i64",
        Type::F32 => "f32",
        Type::F64 => "f64",
        Type::V128 => "v128",
        Type::AnyFunc => "anyfunc",
        Type::AnyRef => "anyref",
        Type::Func | Type::EmptyBlockType => "?",
    }
}
