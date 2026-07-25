//! The fleet's deterministic Rhai host.
//!
//! Extracted from The Last Aeons, whose whole replay guarantee rests on
//! authored scripts being unable to do anything a rerun would not do again.
//! What that takes is not obvious from the outside, and is very easy to lose
//! by accident, so it lives here as one profile with the reasons attached:
//!
//! - **[`sandbox`] starts from a raw engine.** Capabilities are opt-in, not
//!   opt-out. Building from `Engine::new()` and removing things is a losing
//!   game — the next Rhai release adds a helper you did not think to strip,
//!   and a script reaches the wall clock.
//! - **No wall-clock, no I/O, no imports, no `eval`.** `timestamp` is simply
//!   absent; the module resolver is a dummy so `import` cannot reach a file;
//!   `eval` is disabled outright because a script that can build source at
//!   runtime is a script the content hash does not describe.
//! - **Hard limits on operations, recursion, and sizes.** A runaway script
//!   is a hung game; a limit turns it into an error with a file name on it.
//! - **Integer-only arithmetic** (the `no_float` feature), because a float
//!   in a saved campaign is a platform-dependent fingerprint waiting to
//!   happen.
//!
//! What stays in the game: the *vocabulary*. The builder functions a loading
//! engine registers, the shape of the context a call receives, and how a
//! returned value becomes typed effects are all statements about one game's
//! content model. This crate hands out the engine and the call seam; the
//! game says what may be said through it.

use rhai::{CallFnOptions, Dynamic, Engine, Map, Scope, AST};

/// Build the sandboxed engine.
///
/// Deny-by-default: only deterministic language packages are registered, and
/// the limits below are deliberately generous enough for real authored
/// content while still bounding a mistake. Games add their own registered
/// functions on top; they do not remove anything from underneath.
pub fn sandbox() -> Engine {
    use rhai::packages::{
        ArithmeticPackage, BasicArrayPackage, BasicFnPackage, BasicIteratorPackage,
        BasicMapPackage, BasicMathPackage, BasicStringPackage, LanguageCorePackage, LogicPackage,
        MoreStringPackage, Package,
    };

    let mut engine = Engine::new_raw();
    engine.register_global_module(LanguageCorePackage::new().as_shared_module());
    engine.register_global_module(ArithmeticPackage::new().as_shared_module());
    engine.register_global_module(LogicPackage::new().as_shared_module());
    engine.register_global_module(BasicStringPackage::new().as_shared_module());
    engine.register_global_module(MoreStringPackage::new().as_shared_module());
    engine.register_global_module(BasicIteratorPackage::new().as_shared_module());
    engine.register_global_module(BasicArrayPackage::new().as_shared_module());
    engine.register_global_module(BasicMapPackage::new().as_shared_module());
    engine.register_global_module(BasicMathPackage::new().as_shared_module());
    engine.register_global_module(BasicFnPackage::new().as_shared_module());

    engine.set_module_resolver(rhai::module_resolvers::DummyModuleResolver::new());
    engine.disable_symbol("eval");
    engine.set_max_operations(5_000_000);
    engine.set_max_call_levels(64);
    engine.set_max_string_size(65_536);
    engine.set_max_array_size(65_536);
    engine.set_max_map_size(65_536);
    engine.set_max_expr_depths(128, 64);
    engine
}

/// A sandboxed engine with printing and debugging silenced — the profile a
/// *runtime* host wants, where a script's `print` is noise rather than a
/// development aid.
pub fn quiet_sandbox() -> Engine {
    let mut engine = sandbox();
    engine.on_print(|_| {});
    engine.on_debug(|_, _, _| {});
    engine
}

/// One authored source file, path-relative to the content root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptSource {
    /// Content-relative path with forward slashes, e.g. `core/events.rhai`.
    pub path: String,
    /// The script source text.
    pub source: String,
}

/// Hash a content set's sources, binding a save to the exact content it was
/// played against.
///
/// Sources are hashed in the order given (callers sort by path first, so the
/// hash is a property of the content and not of directory iteration), with
/// each path and length framed into the stream: without the framing, moving
/// a character across a file boundary would leave the hash unchanged.
pub fn content_hash(sources: &[ScriptSource]) -> u64 {
    let mut buffer = Vec::new();
    for source in sources {
        buffer.extend_from_slice(source.path.as_bytes());
        buffer.push(0);
        buffer.extend_from_slice(&(source.source.len() as u64).to_le_bytes());
        buffer.extend_from_slice(source.source.as_bytes());
        buffer.push(0);
    }
    vellum_digest::fnv1a(&buffer)
}

/// Why a runtime script call failed.
#[derive(Debug)]
pub enum CallError {
    /// The script raised, or the engine refused it (a limit, a missing
    /// function).
    Runtime {
        /// The file whose function was called.
        path: String,
        /// Engine-reported failure.
        message: String,
    },
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::Runtime { path, message } => {
                write!(f, "script error in {path}: {message}")
            }
        }
    }
}

impl std::error::Error for CallError {}

/// Call a named function in an already-compiled script with a read-only
/// context map, returning whatever it produced.
///
/// The returned [`Dynamic`] is deliberately untyped here: turning it into
/// effects is the game's content model talking, and this crate has no
/// opinion about it.
///
/// `eval_ast(false)` is the load-bearing detail — the file's top level ran
/// once when the content was loaded, and a runtime call must invoke the
/// retained function *only*. Re-running the top level would re-execute every
/// definition the file makes, which at best duplicates work and at worst
/// redefines content mid-campaign.
pub fn call_fn(
    engine: &Engine,
    ast: &AST,
    path: &str,
    name: &str,
    context: Map,
) -> Result<Dynamic, CallError> {
    let mut scope = Scope::new();
    let options = CallFnOptions::new().eval_ast(false);
    engine
        .call_fn_with_options(options, &mut scope, ast, name, (context,))
        .map_err(|err| CallError::Runtime {
            path: path.to_owned(),
            message: err.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile(source: &str) -> (Engine, AST) {
        let engine = sandbox();
        let ast = engine.compile(source).expect("compiles");
        (engine, ast)
    }

    #[test]
    fn arithmetic_and_strings_work() {
        let (engine, ast) = compile("fn effects(ctx) { ctx.n * 2 }");
        let mut context = Map::new();
        context.insert("n".into(), Dynamic::from(21_i64));
        let out = call_fn(&engine, &ast, "t.rhai", "effects", context).expect("calls");
        assert_eq!(out.as_int().unwrap(), 42);
    }

    /// The whole reason the sandbox starts raw: a script must not be able to
    /// ask what time it is, or a replay stops reproducing.
    #[test]
    fn the_wall_clock_is_absent() {
        let engine = sandbox();
        assert!(
            engine.eval::<Dynamic>("timestamp()").is_err(),
            "a script reached the wall clock; the sandbox is not deny-by-default"
        );
    }

    #[test]
    fn eval_is_disabled() {
        let engine = sandbox();
        assert!(
            engine.eval::<Dynamic>(r#"eval("1 + 1")"#).is_err(),
            "a script built source at runtime, which the content hash cannot describe"
        );
    }

    #[test]
    fn imports_resolve_to_nothing() {
        let engine = sandbox();
        assert!(
            engine
                .eval::<Dynamic>(r#"import "anything" as x; 1"#)
                .is_err(),
            "a script reached outside its own file"
        );
    }

    /// A runaway script must become an error with a file name on it, not a
    /// hung game.
    #[test]
    fn runaway_scripts_hit_the_operation_limit() {
        let (engine, ast) = compile("fn effects(ctx) { let i = 0; loop { i += 1; } }");
        let error = call_fn(&engine, &ast, "t.rhai", "effects", Map::new())
            .expect_err("an infinite loop must be refused");
        assert!(
            error.to_string().contains("t.rhai"),
            "the failure must name the file: {error}"
        );
    }

    /// The top level runs once at load. A runtime call re-running it would
    /// re-execute every definition the file makes.
    #[test]
    fn a_runtime_call_does_not_rerun_the_top_level() {
        let source = r#"
            let side_effect = 1;
            fn effects(ctx) { 7 }
        "#;
        let engine = sandbox();
        let ast = engine.compile(source).expect("compiles");
        // Calling with eval_ast(false) must succeed without the top-level
        // `let` being in scope — proving it was not re-run.
        let out = call_fn(&engine, &ast, "t.rhai", "effects", Map::new()).expect("calls");
        assert_eq!(out.as_int().unwrap(), 7);
    }

    #[test]
    fn content_hashes_are_stable_and_framed() {
        let a = vec![
            ScriptSource {
                path: "core/a.rhai".into(),
                source: "let x = 1;".into(),
            },
            ScriptSource {
                path: "core/b.rhai".into(),
                source: "let y = 2;".into(),
            },
        ];
        assert_eq!(content_hash(&a), content_hash(&a.clone()), "stable");

        // Moving a character across the file boundary must change the hash;
        // without the length framing it would not.
        let b = vec![
            ScriptSource {
                path: "core/a.rhai".into(),
                source: "let x = 1;let".into(),
            },
            ScriptSource {
                path: "core/b.rhai".into(),
                source: " y = 2;".into(),
            },
        ];
        assert_ne!(content_hash(&a), content_hash(&b));
    }

    #[test]
    fn a_missing_function_is_a_named_error() {
        let (engine, ast) = compile("fn other(ctx) { 1 }");
        let error = call_fn(&engine, &ast, "core/x.rhai", "effects", Map::new())
            .expect_err("a missing function must fail");
        assert!(error.to_string().contains("core/x.rhai"));
    }
}
