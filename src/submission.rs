use crate::{
    client, errors,
    errors::ToResult,
    image::{language, program},
    problem::{problem, verdict},
    worker,
};
use futures::stream::StreamExt;
use itertools::Itertools;
use multiprocessing::Object;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug, Object)]
pub enum Command {
    Compile(String),
    Test(Vec<u64>),
    Finalize,
}

pub struct Submission {
    pub id: String,
    instantiated_dependency_graph: RwLock<problem::InstantiatedDependencyGraph>,
    language: language::Language,
    source_files: Vec<String>,
    program: RwLock<Option<program::Program>>,
    workers: RwLock<HashMap<u64, Arc<RwLock<worker::Worker>>>>,
    problem_revision: Arc<problem::ProblemRevision>,
    invocation_limits: HashMap<String, verdict::InvocationLimit>,
}

impl Submission {
    pub fn new(
        id: String,
        problem_revision: Arc<problem::ProblemRevision>,
        language: language::Language,
        invocation_limits: HashMap<String, verdict::InvocationLimit>,
    ) -> Result<Submission, errors::Error> {
        let root = format!("/tmp/sunwalker_invoker/submissions/{id}");
        std::fs::create_dir(&root).with_context_invoker(|| {
            format!("Failed to create a directory for submission at {root}")
        })?;

        Ok(Submission {
            id,
            instantiated_dependency_graph: RwLock::new(
                problem_revision.dependency_graph.clone().instantiate(),
            ),
            language,
            source_files: Vec::new(),
            program: RwLock::new(None),
            workers: RwLock::new(HashMap::new()),
            problem_revision,
            invocation_limits,
        })
    }

    pub fn add_source_file(&mut self, name: &str, content: &[u8]) -> Result<(), errors::Error> {
        let path = format!("/tmp/sunwalker_invoker/submissions/{}/{name}", self.id);
        std::fs::write(&path, content).with_context_invoker(|| {
            format!("Failed to write a source code file for submission at {path}")
        })?;
        self.source_files.push(path);
        Ok(())
    }

    async fn execute_on_core(
        &self,
        core: client::CoreHandle,
        command: Command,
        n_messages: usize,
    ) -> Result<impl futures::stream::Stream<Item = worker::W2IMessage>, errors::Error> {
        use std::collections::hash_map::Entry;

        let mut workers = self.workers.write().await;
        let worker = match workers.entry(core.get_core()) {
            Entry::Occupied(occupied) => occupied.get().clone(),
            Entry::Vacant(vacant) => vacant
                .insert(Arc::new(RwLock::new(
                    worker::Worker::new(
                        self.language.clone(),
                        self.source_files.clone(),
                        core.get_core(),
                        self.instantiated_dependency_graph.read().await.clone(),
                        self.program.read().await.clone(),
                        self.problem_revision.strategy_factory.clone(),
                        self.invocation_limits.clone(),
                    )
                    .await?,
                )))
                .clone(),
        };
        drop(workers);

        let worker = worker.read().await;
        worker.execute_command(command, n_messages).await
    }

    pub async fn compile_on_core(&self, core: client::CoreHandle) -> Result<String, errors::Error> {
        if self.program.read().await.is_some() {
            return Err(errors::ConductorFailure(
                "The submission is already compiled".to_string(),
            ));
        }

        let response = self
            .execute_on_core(core, Command::Compile(format!("judge-{}", self.id)), 1)
            .await?
            .next()
            .await;
        match response {
            Some(worker::W2IMessage::CompilationResult(program, log)) => {
                *self.program.write().await = Some(program);
                Ok(log)
            }
            Some(worker::W2IMessage::Failure(e)) => Err(e),
            _ => Err(errors::InvokerFailure(format!(
                "Unexpected response to compilation request: {response:?}"
            ))),
        }
    }

    pub async fn test_on_core(
        &self,
        core: client::CoreHandle,
        tests: Vec<u64>,
    ) -> Result<
        impl futures::stream::Stream<Item = (u64, verdict::TestJudgementResult)>,
        errors::Error,
    > {
        if self.program.read().await.is_none() {
            return Err(errors::ConductorFailure(
                "Cannot judge submission before the program is built".to_string(),
            ));
        }

        let mut i = 0usize;

        Ok(self
            .execute_on_core(core, Command::Test(tests.clone()), tests.len())
            .await?
            .map(move |judgement_result| {
                let test = tests[i];
                i += 1;

                let judgement_result = match judgement_result {
                    worker::W2IMessage::TestResult(result) => Ok(result),
                    worker::W2IMessage::Failure(e) => Err(e),
                    _ => Err(errors::InvokerFailure(format!(
                        "Unexpected response to judgement request: {judgement_result:?}"
                    ))),
                };
                (
                    test,
                    judgement_result.unwrap_or_else(|e| verdict::TestJudgementResult {
                        verdict: verdict::TestVerdict::Bug(format!(
                            "Failed to evaluate test: {e:?}"
                        )),
                        logs: HashMap::new(),
                        invocation_stats: HashMap::new(),
                    }),
                )
            }))
    }

    pub async fn add_failed_tests(&self, tests: &[u64]) -> Result<(), errors::Error> {
        {
            let mut instantiated_dependency_graph =
                self.instantiated_dependency_graph.write().await;
            for test in tests {
                instantiated_dependency_graph.fail_test(*test);
            }
        }
        for (_, worker) in self.workers.read().await.iter() {
            worker
                .read()
                .await
                .add_failed_tests(Vec::from(tests))
                .await?;
        }
        Ok(())
    }

    pub async fn finalize(&self) -> Result<(), errors::Error> {
        if let Some(program) = self.program.write().await.take() {
            program.remove()?;
        }
        let mut workers = self.workers.write().await;

        let mut errors = futures::future::join_all(
            workers
                .iter_mut()
                .map(async move |(_, worker)| worker.write().await.finalize().await),
        )
        .await
        .into_iter()
        .filter_map(|res| res.err())
        .peekable();

        if errors.peek().is_none() {
            Ok(())
        } else {
            Err(errors::InvokerFailure(format!(
                "Failed to finalize submission: {}",
                errors.map(|e| format!("{:?}", e)).join(", ")
            )))
        }
    }
}
