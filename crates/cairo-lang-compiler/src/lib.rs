//! Cairo compiler.
//!
//! This crate is responsible for compiling a Cairo project into a Sierra program.
//! It is the main entry point for the compiler.
use std::path::Path;
use std::sync::Arc;

use ::cairo_lang_diagnostics::ToOption;
use anyhow::{Context, Result};
use cairo_lang_filesystem::ids::CrateId;
use cairo_lang_sierra::program::Program;
use cairo_lang_sierra_generator::db::SierraGenGroup;
use cairo_lang_sierra_generator::program_generator::SierraProgramWithDebug;
use cairo_lang_sierra_generator::replace_ids::replace_sierra_ids_in_program;

use crate::db::RootDatabase;
use crate::diagnostics::DiagnosticsReporter;
use crate::project::{get_main_crate_ids_from_project, setup_project, ProjectConfig};

pub mod db;
pub mod diagnostics;
pub mod project;

/// Configuration for the compiler.
pub struct CompilerConfig<'c> {
    pub diagnostics_reporter: DiagnosticsReporter<'c>,

    /// Replaces sierra ids with human-readable ones.
    pub replace_ids: bool,

    /// The name of the allowed libfuncs list to use in compilation.
    /// If None the default list of audited libfuncs will be used.
    pub allowed_libfuncs_list_name: Option<String>,
}

/// The default compiler configuration.
impl Default for CompilerConfig<'static> {
    fn default() -> Self {
        CompilerConfig {
            diagnostics_reporter: DiagnosticsReporter::default(),
            replace_ids: false,
            allowed_libfuncs_list_name: None,
        }
    }
}

/// Compiles a Cairo project at the given path.
/// The project must be a valid Cairo project:
/// Either a standalone `.cairo` file (a single crate), or a directory with a `cairo_project.toml`
/// file.
/// # Arguments
/// * `path` - The path to the project.
/// * `compiler_config` - The compiler configuration.
/// # Returns
/// * `Ok(Program)` - The compiled program.
/// * `Err(anyhow::Error)` - Compilation failed.
pub fn compile_cairo_project_at_path(
    path: &Path,
    compiler_config: CompilerConfig<'_>,
) -> Result<Program> {
    let mut db = RootDatabase::builder().detect_corelib().build()?;
    let main_crate_ids = setup_project(&mut db, path)?;
    compile_prepared_db_program(&mut db, main_crate_ids, compiler_config)
}

/// Compiles a Cairo project.
/// The project must be a valid Cairo project.
/// This function is a wrapper over [`RootDatabase::builder()`] and [`compile_prepared_db`].
/// # Arguments
/// * `project_config` - The project configuration.
/// * `compiler_config` - The compiler configuration.
/// # Returns
/// * `Ok(Program)` - The compiled program.
/// * `Err(anyhow::Error)` - Compilation failed.
pub fn compile(
    project_config: ProjectConfig,
    compiler_config: CompilerConfig<'_>,
) -> Result<Program> {
    let mut db = RootDatabase::builder().with_project_config(project_config.clone()).build()?;
    let main_crate_ids = get_main_crate_ids_from_project(&mut db, &project_config);

    compile_prepared_db_program(&mut db, main_crate_ids, compiler_config)
}

/// Runs Cairo compiler.
///
/// # Arguments
/// * `db` - Preloaded compilation database.
/// * `main_crate_ids` - [`CrateId`]s to compile. Do not include dependencies here, only pass
///   top-level crates in order to eliminate unused code. Use
///   `db.intern_crate(CrateLongId::Real(name))` in order to obtain [`CrateId`] from its name.
/// * `compiler_config` - The compiler configuration.
/// # Returns
/// * `Ok(Program)` - The compiled program.
/// * `Err(anyhow::Error)` - Compilation failed.
pub fn compile_prepared_db_program(
    db: &mut RootDatabase,
    main_crate_ids: Vec<CrateId>,
    compiler_config: CompilerConfig<'_>,
) -> Result<Program> {
    Ok(compile_prepared_db(db, main_crate_ids, compiler_config)?.program)
}

/// Runs Cairo compiler.
///
/// Similar to `compile_prepared_db_program`, but this function returns all the raw debug
/// information.
///
/// # Arguments
/// * `db` - Preloaded compilation database.
/// * `main_crate_ids` - [`CrateId`]s to compile. Do not include dependencies here, only pass
///   top-level crates in order to eliminate unused code. Use
///   `db.intern_crate(CrateLongId::Real(name))` in order to obtain [`CrateId`] from its name.
/// * `compiler_config` - The compiler configuration.
/// # Returns
/// * `Ok(SierraProgramWithDebug)` - The compiled program with debug info.
/// * `Err(anyhow::Error)` - Compilation failed.
pub fn compile_prepared_db(
    db: &mut RootDatabase,
    main_crate_ids: Vec<CrateId>,
    mut compiler_config: CompilerConfig<'_>,
) -> Result<SierraProgramWithDebug> {
    compiler_config.diagnostics_reporter.ensure(db)?;

    let mut sierra_program_with_debug = Arc::unwrap_or_clone(
        db.get_sierra_program(main_crate_ids)
            .to_option()
            .context("Compilation failed without any diagnostics")?,
    );

    if compiler_config.replace_ids {
        sierra_program_with_debug.program =
            replace_sierra_ids_in_program(db, &sierra_program_with_debug.program);
    }

    Ok(sierra_program_with_debug)
}
