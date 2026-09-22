//! Model factories — Laravel's `User::factory()->count(3)->create()`.
//!
//! A factory is a recipe for a valid row. Implement [`Factory::definition`]
//! once per model, then build as many variations as a test (or a seeder) needs:
//!
//! ```ignore
//! impl Factory for User {
//!     fn definition(n: u64) -> Self {
//!         User { id: 0, name: format!("User {n}"), email: format!("user{n}@example.test"), admin: false }
//!     }
//! }
//!
//! let users = User::factory().count(3).create(&db).await?;          // inserted
//! let admin = User::factory().state(|u| u.admin = true).create_one(&db).await?;
//! let draft = User::factory().make_one();                            // not inserted
//!
//! // Related rows: a factory for the child, pointed at the parent.
//! Post::factory().count(5).state(move |p| p.user_id = admin.id).create(&db).await?;
//! ```
//!
//! `n` is unique for the life of the process (across every model and every
//! builder), so it is safe to use for columns with a unique constraint.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::Result;
use crate::model::Persist;
use crate::Database;

/// The process-wide sequence handed to [`Factory::definition`].
static SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// A recipe for valid instances of a model.
pub trait Factory: Persist {
    /// A fresh, valid instance. `n` is unique within the process — build unique
    /// values (emails, slugs) from it.
    fn definition(n: u64) -> Self;

    /// Start building instances.
    fn factory() -> FactoryBuilder<Self> {
        FactoryBuilder::new()
    }
}

type StateFn<M> = Box<dyn Fn(&mut M, usize) + Send + Sync>;

/// Configures how many instances to build and how they differ from the
/// [`definition`](Factory::definition). States apply in the order they were
/// added, after the definition.
pub struct FactoryBuilder<M> {
    count: usize,
    states: Vec<StateFn<M>>,
}

impl<M: Factory> Default for FactoryBuilder<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Factory> FactoryBuilder<M> {
    pub fn new() -> Self {
        Self {
            count: 1,
            states: Vec::new(),
        }
    }

    /// How many instances [`make`](Self::make) / [`create`](Self::create)
    /// produce. Defaults to 1.
    pub fn count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }

    /// Override fields on every instance — Laravel's `state([...])`.
    pub fn state(mut self, state: impl Fn(&mut M) + Send + Sync + 'static) -> Self {
        self.states.push(Box::new(move |model, _| state(model)));
        self
    }

    /// Override fields per instance, given its index within this batch (0-based)
    /// — Laravel's `sequence(..)`, e.g. alternating roles.
    pub fn sequence(mut self, state: impl Fn(&mut M, usize) + Send + Sync + 'static) -> Self {
        self.states.push(Box::new(state));
        self
    }

    fn build(&self, index: usize) -> M {
        let mut model = M::definition(SEQUENCE.fetch_add(1, Ordering::Relaxed));
        for state in &self.states {
            state(&mut model, index);
        }
        model
    }

    /// Build `count` instances without touching the database.
    pub fn make(&self) -> Vec<M> {
        (0..self.count).map(|i| self.build(i)).collect()
    }

    /// Build one instance without touching the database (ignores `count`).
    pub fn make_one(&self) -> M {
        self.build(0)
    }

    /// Build and insert `count` instances, returning them with their keys set.
    pub async fn create(&self, db: &Database) -> Result<Vec<M>> {
        let mut models = self.make();
        for model in &mut models {
            Persist::insert(model, db).await?;
        }
        Ok(models)
    }

    /// Build and insert one instance (ignores `count`).
    pub async fn create_one(&self, db: &Database) -> Result<M> {
        let mut model = self.make_one();
        Persist::insert(&mut model, db).await?;
        Ok(model)
    }
}
