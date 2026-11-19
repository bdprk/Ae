"""Idempotent patch: wire the ADR-0215 deferred spawn-escape proofs."""
import io

def apply(path, edits):
    with io.open(path, 'r', encoding='utf-8') as f:
        t = f.read()
    changed = False
    for old, new in edits:
        if old in t:
            t = t.replace(old, new, 1)
            changed = True
            print(f'  {path}: applied')
        elif new in t:
            print(f'  {path}: already applied')
        else:
            print(f'  {path}: NEITHER PRESENT NOR APPLIED — {old[:60]!r}')
    if changed:
        with io.open(path, 'w', encoding='utf-8', newline='\n') as f:
            f.write(t)

apply('crates/ae-compiler/src/frontend/borrowscope.rs', [
    ("""    for (handle, _borrowed, _offset) in spawns {
        // The joins: blocks containing a `wait` of this handle; the spawn
        // site: the `Declare` binding the handle from a spawn expression.
        let mut join_blocks: Vec<usize> = Vec::new();
        let mut spawn_site: Option<(usize, usize)> = None;
        for block in &graph.blocks {
            for (index, statement) in block.statements.iter().enumerate() {
                if wait_targets(statement).contains(handle) {
                    join_blocks.push(block.id);
                }
                if let Statement::Declare(id, expr) = statement {
                    if id == handle {
                        if matches!(expr, Expr::Spawn(..)) {
                            spawn_site = Some((block.id, index + 1));
                        }
                    }
                }
            }
        }
        let Some((spawn_block, spawn_index)) = spawn_site else {
            return Err("internal: spawn site of a borrowed-capture thread vanished".to_owned());
        };""",
     """    for (cid, _borrowed, _offset) in spawns {
        // The joins: blocks containing a `wait` of this spawn's handle; the
        // spawn site: the `Declare` binding a handle from THIS closure's
        // spawn expression. A borrow-capture spawn that never binds a handle
        // cannot be waited and is rejected outright.
        let mut join_blocks: Vec<usize> = Vec::new();
        let mut spawn_site: Option<(usize, usize, usize)> = None;
        for block in &graph.blocks {
            for (index, statement) in block.statements.iter().enumerate() {
                if let Statement::Declare(id, expr) = statement {
                    if let Expr::Spawn(_, callable, ..) = expr {
                        let spawned_here = match &**callable {
                            Expr::ClosureLit(lit) => lit == cid,
                            Expr::Variable(_, Type::Closure(lit), ..) => lit == cid,
                            _ => false,
                        };
                        if spawned_here {
                            spawn_site = Some((block.id, index + 1, *id));
                        }
                    }
                }
            }
        }
        let Some((spawn_block, spawn_index, handle)) = spawn_site else {
            return Err("a spawn with borrowed captures must bind its handle to a name and wait it; an unnamed handle could never be joined".to_owned());
        };
        for block in &graph.blocks {
            for statement in &block.statements {
                if wait_targets(statement).contains(&handle) {
                    join_blocks.push(block.id);
                }
            }
        }"""),
])

apply('crates/ae-compiler/src/frontend/mod.rs', [
    ('deferred_borrow_spawns: Vec<(usize, Vec<usize>, usize, usize)>',
     'deferred_borrow_spawns: Vec<(usize, Vec<usize>, usize)>'),
    ('deferred_write_conflicts: Vec::new(), call_borrows:',
     'deferred_write_conflicts: Vec::new(), deferred_borrow_spawns: Vec::new(), call_borrows:'),
])

apply('crates/ae-compiler/src/frontend/expression.rs', [
    ("""                            if !def.borrow_captures.is_empty() {
                                let wanted = def.borrow_captures[0].0;
                                let name = self.names.iter()
                                    .find(|(_, entry)| entry.0 == wanted)
                                    .map(|(name, _)| name.clone())
                                    .unwrap_or_else(|| "a variable".to_owned());
                                return Err(self.error_at(at, &format!("spawn moves its captures into the thread; '{name}' is captured by reference and cannot cross")));
                            }""",
     """                            // ADR-0215: borrow captures are accepted when the
                            // handle is provably joined before the borrowed
                            // region ends — the escape proof defers to the
                            // function's CFG. Until the proof lands, the
                            // borrowed variables freeze like views (no write,
                            // no move), which is sound on its own.
                            if !def.borrow_captures.is_empty() {
                                let borrowed: Vec<usize> = def.borrow_captures.iter().map(|(outer_id, _)| *outer_id).collect();
                                let depth = self.scopes.len();
                                for outer_id in &borrowed {
                                    self.borrows.push((*outer_id, depth, BorrowKind::View));
                                }
                                self.deferred_borrow_spawns.push((*cid, borrowed, at));
                            }"""),
])

apply('crates/ae-compiler/src/frontend/parser.rs', [
    ("""    pub(super) fn adjudicate_deferred_conflicts(&mut self, function: &Function) -> Result<(), ae_span::Diagnostic> {
        let mut_sigs = borrowscope::mut_borrow_positions(self.functions, self.generics);
        let outcome = borrowscope::adjudicate_deferred(function, &self.deferred_write_conflicts, &mut_sigs);
        match outcome {
            Ok(()) => { self.deferred_write_conflicts.clear(); Ok(()) }
            Err(message) => {
                let (_, offset, _, _, _) = self.deferred_write_conflicts[0];
                Err(self.error_at(offset, &message))
            }
        }
    }""",
     """    pub(super) fn adjudicate_deferred_conflicts(&mut self, function: &Function) -> Result<(), ae_span::Diagnostic> {
        let mut_sigs = borrowscope::mut_borrow_positions(self.functions, self.generics);
        let outcome = borrowscope::adjudicate_deferred(function, &self.deferred_write_conflicts, &mut_sigs);
        match outcome {
            Ok(()) => { self.deferred_write_conflicts.clear(); Ok(()) }
            Err(message) => {
                let (_, offset, _, _, _) = self.deferred_write_conflicts[0];
                Err(self.error_at(offset, &message))
            }
        }
    }
    // ADR-0215: the deferred spawn-escape proofs (closure id, the borrowed
    // variable ids, the spawn's byte offset).
    pub(super) fn adjudicate_borrow_spawns(&mut self, function: &Function) -> Result<(), ae_span::Diagnostic> {
        let outcome = borrowscope::adjudicate_borrow_spawns(function, &self.deferred_borrow_spawns);
        match outcome {
            Ok(()) => { self.deferred_borrow_spawns.clear(); Ok(()) }
            Err(message) => {
                let (_, _, offset) = self.deferred_borrow_spawns[0];
                Err(self.error_at(offset, &message))
            }
        }
    }"""),
])
