use tauri::{AppHandle, Emitter, State};
use super::{chat::ChatState, http::{codes, RoadeepError}, Roadeep};
use crate::{log, settings, Shared};

#[tauri::command]
pub async fn chat_model_set(
    app: AppHandle,
    shared: State<'_, Shared>,
    chat: State<'_, ChatState>,
    roadeep: State<'_, Roadeep>,
    model: String,
) -> Result<String, RoadeepError> {
    if !roadeep.session.has_session() {
        return Err(RoadeepError::new(codes::NOT_SIGNED_IN, "Sign in to choose a text model."));
    }
    let updated = chat.change_model(&model, &shared.settings, |settings| settings::save(settings).map_err(|_| ()))?;
    if app.emit("settings-changed", &updated).is_err() {
        log::line("chat: could not broadcast the saved model choice");
    }
    Ok(updated.model)
}
