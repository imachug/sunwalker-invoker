use crate::{errors, errors::ToResult, image::strategy};
use multiprocessing::Object;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Deserialize, Serialize)]
pub struct ProblemRevision {
    pub dependency_graph: DependencyGraph,
    pub strategy_factory: strategy::StrategyFactory,
}

#[derive(Object, Clone, Deserialize, Serialize)]
pub struct DependencyGraph {
    pub dependents_of: HashMap<u64, Vec<u64>>,
}

#[derive(Object, Clone)]
pub struct InstantiatedDependencyGraph {
    pub graph: DependencyGraph,
    pub disabled_tests: HashSet<u64>,
}

impl ProblemRevision {
    pub fn load_from_cache(path: &Path) -> Result<Self, errors::Error> {
        let config = std::fs::read(path.join("judging.msgpack")).with_context_invoker(|| {
            format!("Could not read judging.msgpack to load problem from cache at {path:?}")
        })?;

        let mut config: Self = rmp_serde::from_slice(&config).map_err(|e| {
            errors::ConfigurationFailure(format!(
                "Failed to parse judging.msgpack to load problem from cache at {path:?}: {e:?}"
            ))
        })?;

        config.strategy_factory.root = path.to_owned();

        Ok(config)
    }
}

impl DependencyGraph {
    pub fn instantiate(self) -> InstantiatedDependencyGraph {
        InstantiatedDependencyGraph {
            graph: self,
            disabled_tests: HashSet::new(),
        }
    }
}

impl InstantiatedDependencyGraph {
    fn _fail_test(
        dependents_of: &HashMap<u64, Vec<u64>>,
        disabled_tests: &mut HashSet<u64>,
        test: u64,
    ) {
        if !disabled_tests.contains(&test) {
            disabled_tests.insert(test);
            for dep_test in dependents_of.get(&test).unwrap_or(&Vec::new()).iter() {
                Self::_fail_test(dependents_of, disabled_tests, *dep_test)
            }
        }
    }

    pub fn fail_test(&mut self, test: u64) {
        Self::_fail_test(&self.graph.dependents_of, &mut self.disabled_tests, test)
    }

    pub fn is_test_enabled(&self, test: u64) -> bool {
        !self.disabled_tests.contains(&test)
    }
}
