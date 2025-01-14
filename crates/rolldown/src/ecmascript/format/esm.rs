use arcstr::ArcStr;
use itertools::Itertools;
use rolldown_common::{ChunkKind, ExportsKind, Module, WrapKind};
use rolldown_sourcemap::SourceJoiner;

use crate::{
  ecmascript::ecma_generator::RenderedModuleSources,
  types::generator::GenerateContext,
  utils::chunk::{
    collect_render_chunk_imports::{
      collect_render_chunk_imports, RenderImportDeclarationSpecifier,
    },
    render_chunk_exports::render_chunk_exports,
  },
};

pub fn render_esm<'code>(
  ctx: &mut GenerateContext<'_>,
  module_sources: &'code RenderedModuleSources,
  banner: Option<&'code str>,
  footer: Option<&'code str>,
  intro: Option<&'code str>,
  outro: Option<&'code str>,
  hashbang: Option<&'code str>,
) -> SourceJoiner<'code> {
  let mut source_joiner = SourceJoiner::default();

  if let Some(hashbang) = hashbang {
    source_joiner.append_source(hashbang);
  }

  if let Some(banner) = banner {
    source_joiner.append_source(banner);
  }

  if let Some(intro) = intro {
    source_joiner.append_source(intro);
  }

  source_joiner.append_source(render_esm_chunk_imports(ctx));

  if let ChunkKind::EntryPoint { module: entry_id, .. } = ctx.chunk.kind {
    if let Module::Normal(entry_module) = &ctx.link_output.module_table.modules[entry_id] {
      if matches!(entry_module.exports_kind, ExportsKind::Esm) {
        entry_module
          .star_export_module_ids()
          .filter_map(|importee| {
            let importee = &ctx.link_output.module_table.modules[importee];
            match importee {
              Module::External(ext) => Some(&ext.name),
              Module::Normal(_) => None,
            }
          })
          .dedup()
          .for_each(|ext_name| {
            let import_stmt = format!("export * from \"{}\"\n", &ext_name);
            source_joiner.append_source(import_stmt);
          });
      }
    }
  }

  // chunk content
  module_sources.iter().for_each(|(_, _, module_render_output)| {
    if let Some(emitted_sources) = module_render_output {
      for source in emitted_sources {
        source_joiner.append_source(source);
      }
    }
  });

  if let ChunkKind::EntryPoint { module: entry_id, .. } = ctx.chunk.kind {
    let entry_meta = &ctx.link_output.metas[entry_id];
    match entry_meta.wrap_kind {
      WrapKind::Esm => {
        // init_xxx()
        let wrapper_ref = entry_meta.wrapper_ref.as_ref().unwrap();
        let wrapper_ref_name =
          ctx.link_output.symbol_db.canonical_name_for(*wrapper_ref, &ctx.chunk.canonical_names);
        source_joiner.append_source(format!("{wrapper_ref_name}();",));
      }
      WrapKind::Cjs => {
        // "export default require_xxx();"
        let wrapper_ref = entry_meta.wrapper_ref.as_ref().unwrap();
        let wrapper_ref_name =
          ctx.link_output.symbol_db.canonical_name_for(*wrapper_ref, &ctx.chunk.canonical_names);
        source_joiner.append_source(format!("export default {wrapper_ref_name}();\n"));
      }
      WrapKind::None => {}
    }
  }

  if let Some(exports) = render_chunk_exports(ctx, None) {
    if !exports.is_empty() {
      source_joiner.append_source(exports);
    }
  }

  if let Some(outro) = outro {
    source_joiner.append_source(outro);
  }

  if let Some(footer) = footer {
    source_joiner.append_source(footer);
  }

  source_joiner
}

fn render_esm_chunk_imports(ctx: &GenerateContext<'_>) -> String {
  let render_import_stmts =
    collect_render_chunk_imports(ctx.chunk, ctx.link_output, ctx.chunk_graph);

  let mut s = String::new();
  render_import_stmts.iter().for_each(|stmt| {
    let path = stmt.path();
    match &stmt.specifiers() {
      RenderImportDeclarationSpecifier::ImportSpecifier(specifiers) => {
        if specifiers.is_empty() {
          s.push_str(&format!("import \"{path}\";\n",));
        } else {
          let mut default_alias = vec![];
          let specifiers = specifiers
            .iter()
            .filter_map(|specifier| {
              if let Some(alias) = &specifier.alias {
                if specifier.imported == "default" {
                  default_alias.push(alias.to_string());
                  return None;
                }
                Some(format!("{} as {alias}", specifier.imported))
              } else {
                Some(specifier.imported.to_string())
              }
            })
            .collect::<Vec<_>>();
          s.push_str(&create_import_declaration(specifiers, &default_alias, path));
        }
      }
      RenderImportDeclarationSpecifier::ImportStarSpecifier(alias) => {
        s.push_str(&format!("import * as {alias} from \"{path}\";\n",));
      }
    }
  });
  s
}

fn create_import_declaration(
  mut specifiers: Vec<String>,
  default_alias: &[String],
  path: &ArcStr,
) -> String {
  let mut ret = String::new();
  let first_default_alias = match &default_alias {
    [] => None,
    [first] => Some(first),
    [first, rest @ ..] => {
      specifiers.extend(rest.iter().map(|item| format!("default as {item}",)));
      Some(first)
    }
  };
  if !specifiers.is_empty() {
    ret.push_str("import ");
    if let Some(first_default_alias) = first_default_alias {
      ret.push_str(first_default_alias);
      ret.push_str(", ");
    }
    ret.push_str(&format!("{{ {} }} from \"{path}\";\n", specifiers.join(", ")));
  } else if let Some(first_default_alias) = first_default_alias {
    ret.push_str(&format!("import {first_default_alias} from \"{path}\";\n"));
  }
  ret
}
