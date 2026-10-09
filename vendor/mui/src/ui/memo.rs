//! [`Ui::memo`]: subtrees kept between frames.
use super::*;
use mui_scene::Id;

impl Ui {
    /// A subtree built only when `deps` changes: while `deps` equals what
    /// it was last frame, and nothing inside moved, `build` does not run
    /// and the runtime reuses the subtree it kept -- styled, laid out and
    /// painted -- without walking it again.
    ///
    /// "Moved" is the runtime's own state, tracked by the keys the subtree
    /// painted: a hover, press, capture or focus arriving or leaving (call
    /// [`Ui::anticipate`] with the pointer before building, or a hover lands
    /// on the frame after), the
    /// canvas shape under the pointer, a spring, transition, glide, morph or
    /// scroll still in flight, a tween or play it read still running, and
    /// any button, key, text or wheel input, which every tree reads. The
    /// frame after one of those the closure runs again.
    ///
    /// `deps` is the caller's side of it and must cover everything else
    /// `build` reads: the model, sizes, the clock, the raw pointer, and the
    /// enclosing subtree's disabled state. A cheap hash of generation
    /// counters is the idea; a missed input paints last frame's subtree.
    /// What the caller does to the returned element (a size, a cursor) is
    /// kept with the subtree: fold it into `deps` too, or do it inside
    /// `build`. `id` must be unique among the frame's memos. Memos nest. A memo
    /// whose subtree floats a node (a popup) is built every frame: memoise
    /// what is under the float instead.
    ///
    /// ```
    /// # use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut built = 0;
    /// for _ in 0..3 {
    ///     let side = ui.memo("side", 7, |_| {
    ///         built += 1;
    ///         col([block(40., 40.).id("a"), block(40., 40.).id("b")])
    ///     });
    ///     ui.frame(row([side]), Some(Size::new(100., 100.)), PointerInput::default(), 0.016)
    ///         .unwrap();
    /// }
    /// assert_eq!(built, 1);
    /// ```
    pub fn memo(
        &mut self,
        id: impl Into<Id>,
        deps: impl std::hash::Hash,
        build: impl FnOnce(&mut Self) -> El,
    ) -> El {
        let id: Id = id.into();
        let id = id.as_str();
        // ponytail: `deps` compares by its SipHash, so two deps that collide
        // share a subtree; pass something `Eq` and store it if that matters.
        let deps = {
            let mut h = std::hash::DefaultHasher::new();
            deps.hash(&mut h);
            std::hash::Hasher::finish(&h)
        };
        // The scene carries a memo as a number: each id gets its own for as
        // long as its memo is kept, so two ids never share one.
        let next = &mut self.next_memo;
        let key = *(self.memo_ids)
            .entry(mui_scene::Id::runtime(id))
            .or_insert_with(|| {
                *next += 1;
                *next
            });
        self.issued += 1;
        let cold = self.hot_all || self.hot.contains(&key);
        if let Some(k) = self.kept.get_mut(&key) {
            let me = self.me;
            let kept = TREES.with(|t| t.borrow().contains_key(&(me, key)));
            if !cold && k.deps == deps && k.still && kept {
                k.live = true;
                let read = std::mem::take(&mut k.read);
                // What it read is still read, as the closure would have.
                for id in &read {
                    if let Some(n) = self.nodes.get_mut(id.as_str()) {
                        n.tween.iter_mut().for_each(|t| t.0 = true);
                        n.play.iter_mut().for_each(|p| p.0 = true);
                    }
                }
                // Reads stay on the building ones around it too.
                if !self.building.is_empty() {
                    self.read.extend(read.iter().cloned());
                }
                self.kept.get_mut(&key).expect("just read").read = read;
                return placeholder(key);
            }
        }
        self.building.push((key, self.read.len(), true));
        let mut el = build(self);
        let (_, from, still) = self.building.pop().expect("pushed above");
        let read = self.read[from..].to_vec();
        if self.building.is_empty() {
            self.read.clear();
        }
        debug_assert!(
            el.payload().extras().memo.is_none(),
            "a memo's root cannot be another memo's root"
        );
        el.payload_mut().extras_mut().memo = Some(mui_scene::Memo {
            id: key,
            reused: false,
        });
        let me = self.me;
        TREES.with(|t| t.borrow_mut().remove(&(me, key)));
        self.kept.insert(
            key,
            Kept {
                id: mui_scene::Id::runtime(id),
                deps,
                nested: Vec::new(),
                read,
                still,
                live: true,
            },
        );
        el
    }
    /// Put every kept memo subtree back where the build left its
    /// placeholder, and return where each memo's root is, by child-index
    /// path. Walks only until it has met every root the build handed out.
    /// Check the tree as it will appear after memo expansion without moving
    /// retained trees or descending beyond the frame's depth budget.
    pub(super) fn validate_expanded_tree_budget(&mut self, root: &El) -> Result<(), SceneError> {
        if self.issued == 0 {
            return Ok(());
        }
        let limits = mui_layout::Limits::default();
        self.expanded_memos.clear();
        TREES.with(|trees| {
            let trees = trees.borrow();
            Self::validate_tree_budget_with(root, limits, |node| {
                if let Some(memo) = node.payload().extras().memo
                    && memo.reused
                    && self.expanded_memos.insert(memo.id)
                    && let Some(tree) = trees.get(&(self.me, memo.id))
                {
                    tree
                } else {
                    node
                }
            })
        })
    }

    pub(super) fn splice(&mut self, root: &mut El) -> Vec<(Vec<usize>, u64)> {
        let mut found = Vec::new();
        let mut left = std::mem::take(&mut self.issued);
        if left > 0 {
            self.seek(root, &mut Vec::new(), &mut found, &mut left);
        }
        found
    }
    /// [`Ui::splice`] below `n`. Returns whether every root has been met.
    pub(super) fn seek(
        &mut self,
        n: &mut El,
        path: &mut Vec<usize>,
        found: &mut Vec<(Vec<usize>, u64)>,
        left: &mut usize,
    ) -> bool {
        if let Some(m) = n.payload().extras().memo {
            *left -= 1;
            if m.reused {
                self.fill(n, path, m.id, found);
                return *left == 0;
            }
            found.push((path.clone(), m.id));
        }
        for (j, c) in n.children_mut().iter_mut().enumerate() {
            if *left == 0 {
                break;
            }
            path.push(j);
            self.seek(c, path, found, left);
            path.pop();
        }
        *left == 0
    }
    /// Swap placeholder `n` for kept memo `id`, and its nested ones into it.
    pub(super) fn fill(
        &mut self,
        n: &mut El,
        path: &[usize],
        id: u64,
        found: &mut Vec<(Vec<usize>, u64)>,
    ) {
        let Some(k) = self.kept.get_mut(&id) else {
            return;
        };
        let me = self.me;
        let Some(tree) = TREES.with(|t| t.borrow_mut().remove(&(me, id))) else {
            return;
        };
        k.live = true;
        let nested = std::mem::take(&mut k.nested);
        *n = tree;
        if let Some(m) = &mut n.payload_mut().extras_mut().memo {
            m.reused = true;
        }
        found.push((path.to_vec(), id));
        for (rel, inner) in nested {
            let at: Vec<usize> = path.iter().chain(&rel).copied().collect();
            self.fill(node_at(n, &rel), &at, inner, found);
        }
    }
    /// Take every memo's styled subtree back out of the resolved tree,
    /// innermost first, leaving placeholders for nested ones.
    pub(super) fn capture(&mut self, mut root: El, found: &[(Vec<usize>, u64)]) {
        // Each root's nearest enclosing root.
        let parent = |p: &[usize]| {
            found
                .iter()
                .enumerate()
                .filter(|(_, (q, _))| q.len() < p.len() && p.starts_with(q))
                .max_by_key(|(_, (q, _))| q.len())
                .map(|(i, _)| i)
        };
        let parents: Vec<Option<usize>> = found.iter().map(|(p, _)| parent(p)).collect();
        let mut order: Vec<usize> = (0..found.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(found[i].0.len()));
        for i in order {
            let (path, id) = &found[i];
            let tree = std::mem::replace(node_at(&mut root, path), placeholder(*id));
            let nested = found
                .iter()
                .zip(&parents)
                .filter(|(_, p)| **p == Some(i))
                .map(|((q, c), _)| (q[path.len()..].to_vec(), *c))
                .collect();
            if let Some(k) = self.kept.get_mut(id) {
                k.nested = nested;
                TREES.with(|t| t.borrow_mut().insert((self.me, *id), tree));
            }
        }
    }
}

thread_local! {
    /// The kept [`Ui::memo`] subtrees by `(Ui::me, memo number)`. An `El`
    /// holds closures that are not `Send` and a `Ui` must be: the trees stay
    /// on the thread that built them, and a `Ui` that moves thread builds
    /// its memos over.
    ///
    /// ponytail: one table per thread, shared by every `Ui` on it and
    /// probed per memo per frame. Move it onto `Ui` if `El` ever becomes
    /// `Send`, or if a host runs many `Ui`s on one thread.
    pub(super) static TREES: std::cell::RefCell<HashMap<(u64, u64), El>> =
        std::cell::RefCell::new(HashMap::new());
}
pub(super) static NEXT_UI: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Drop for Ui {
    fn drop(&mut self) {
        let me = self.me;
        let _ = TREES.try_with(|t| t.borrow_mut().retain(|(ui, _), _| *ui != me));
    }
}

/// One [`Ui::memo`] subtree between frames.
pub(super) struct Kept {
    /// The id it was built under, whose number it holds in `memo_ids`.
    pub(super) id: mui_scene::Id,
    pub(super) deps: u64,
    /// Its styled subtree, last frame's, sits in [`TREES`] while it is out
    /// of the tree. Memos nested in it are kept on their own, and it holds
    /// placeholders where they go: their paths below it, and ids.
    pub(super) nested: Vec<(Vec<usize>, u64)>,
    /// The tweens and plays its closure read, and whether every one of them
    /// was at rest when it did.
    pub(super) read: Vec<String>,
    pub(super) still: bool,
    /// Handed out, or brought back inside one that was, this frame.
    pub(super) live: bool,
}

/// What [`Ui::memo`] hands back for a subtree it kept: the frame puts the
/// kept one in its place.
pub(super) fn placeholder(id: u64) -> El {
    let mut el = mui_scene::block(0.0, 0.0);
    el.payload_mut().extras_mut().memo = Some(mui_scene::Memo { id, reused: true });
    el
}

/// The node at child-index `path` below `n`.
pub(super) fn node_at<'a>(n: &'a mut El, path: &[usize]) -> &'a mut El {
    path.iter().fold(n, |n, &j| &mut n.children_mut()[j])
}
