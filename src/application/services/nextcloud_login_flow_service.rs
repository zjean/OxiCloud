use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand_core::RngCore;

#[derive(Debug, Clone)]
pub struct LoginFlowInfo {
    pub poll_token: String,
    pub poll_endpoint: String,
    pub login_url: String,
}

#[derive(Debug, Clone)]
pub struct LoginResult {
    pub server: String,
    pub login_name: String,
    pub app_password: String,
}

#[derive(Debug)]
struct PendingFlow {
    created_at: Instant,
    poll_token: String,
    completed: Option<LoginResult>,
}

#[derive(Default)]
struct FlowState {
    flows: HashMap<String, PendingFlow>,
    poll_to_flow: HashMap<String, String>,
}

#[derive(Clone)]
pub struct NextcloudLoginFlowService {
    ttl: Duration,
    state: Arc<Mutex<FlowState>>,
}

impl NextcloudLoginFlowService {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            state: Arc::new(Mutex::new(FlowState::default())),
        }
    }

    pub fn new_stub() -> Self {
        Self::new(Duration::from_secs(600))
    }

    pub fn initiate(&self, base_url: &str) -> LoginFlowInfo {
        let mut state = self.state.lock().expect("login flow lock");
        prune_expired(&mut state, self.ttl);

        let poll_token = random_hex(64);
        let flow_token = random_hex(48);

        state.poll_to_flow.insert(poll_token.clone(), flow_token.clone());
        state.flows.insert(
            flow_token.clone(),
            PendingFlow {
                created_at: Instant::now(),
                poll_token: poll_token.clone(),
                completed: None,
            },
        );

        LoginFlowInfo {
            poll_token: poll_token.clone(),
            poll_endpoint: format!("{}/login/v2/poll", base_url.trim_end_matches('/')),
            login_url: format!("{}/login/v2/flow/{}", base_url.trim_end_matches('/'), flow_token),
        }
    }

    pub fn flow_exists(&self, flow_token: &str) -> bool {
        let mut state = self.state.lock().expect("login flow lock");
        prune_expired(&mut state, self.ttl);
        state.flows.contains_key(flow_token)
    }

    pub fn complete(
        &self,
        flow_token: &str,
        username: &str,
        server: &str,
        app_password: &str,
    ) -> bool {
        let mut state = self.state.lock().expect("login flow lock");
        prune_expired(&mut state, self.ttl);

        let pending = match state.flows.get_mut(flow_token) {
            Some(pending) => pending,
            None => return false,
        };

        pending.completed = Some(LoginResult {
            server: server.to_string(),
            login_name: username.to_string(),
            app_password: app_password.to_string(),
        });

        true
    }

    pub fn poll(&self, poll_token: &str) -> Option<LoginResult> {
        let mut state = self.state.lock().expect("login flow lock");
        prune_expired(&mut state, self.ttl);

        let flow_token = state.poll_to_flow.get(poll_token).cloned()?;
        let pending = state.flows.get_mut(&flow_token)?;

        if let Some(result) = pending.completed.take() {
            state.poll_to_flow.remove(poll_token);
            state.flows.remove(&flow_token);
            Some(result)
        } else {
            None
        }
    }
}

fn prune_expired(state: &mut FlowState, ttl: Duration) {
    let now = Instant::now();
    let expired: Vec<String> = state
        .flows
        .iter()
        .filter(|(_, flow)| now.duration_since(flow.created_at) > ttl)
        .map(|(token, _)| token.clone())
        .collect();

    for flow_token in expired {
        if let Some(flow) = state.flows.remove(&flow_token) {
            state.poll_to_flow.remove(&flow.poll_token);
        }
    }
}

fn random_hex(len: usize) -> String {
    let mut bytes = vec![0u8; (len + 1) / 2];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let mut out = hex::encode(bytes);
    out.truncate(len);
    out
}
