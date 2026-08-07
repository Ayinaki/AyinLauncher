use super::model::{
    InstallJobSnapshot, InstallJobState, InstallPhaseDetails, InstallPhaseId,
    InstallProgress,
};
use super::store;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct InstallProgressReporter {
    job_id: Uuid,
    state: Arc<Mutex<InstallJobState>>,
    last_emitted: Arc<Mutex<Option<Instant>>>,
}

impl InstallProgressReporter {
    pub fn new(job_id: Uuid, state: InstallJobState) -> Self {
        Self {
            job_id,
            state: Arc::new(Mutex::new(state)),
            last_emitted: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn update(
        &self,
        phase: InstallPhaseId,
        progress: Option<InstallProgress>,
        details: InstallPhaseDetails,
    ) -> crate::Result<()> {
        let app_state = crate::State::get().await?;
        let mut state = self.state.lock().await;

        let mut should_emit = false;
        let mut last_emitted = self.last_emitted.lock().await;
        let now = Instant::now();

        if state.progress.phase != phase {
            should_emit = true;
        } else if let Some(last) = *last_emitted {
            if now.duration_since(last).as_millis() >= 100 {
                should_emit = true;
            }
        } else {
            should_emit = true;
        }

        state.progress.phase = phase;
        state.progress.progress = progress;
        state.progress.details = details;

        if should_emit {
            *last_emitted = Some(now);
            let record =
                store::update_state(self.job_id, &state, &app_state).await?;
            emit_install_job(&record.snapshot()).await?;
        }

        Ok(())
    }
}

#[allow(unused_variables)]
pub async fn emit_install_job(
    snapshot: &InstallJobSnapshot,
) -> crate::Result<()> {
    #[cfg(feature = "tauri")]
    {
        use tauri::Emitter;

        let event_state = crate::EventState::get()?;
        event_state
            .app
            .emit("install_job", snapshot)
            .map_err(crate::event::EventError::from)?;
    }

    Ok(())
}
