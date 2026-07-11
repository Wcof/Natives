# Provider Model Discovery and Assistant Availability Design

## Context

The provider setup flow, provider connection test, and Assistant V2 currently do not share one authoritative model selection path. The add-provider dialog contains hard-coded test model IDs, discovered models are only temporary UI state, and Assistant V2 can report that no provider or model is available even after a provider was configured in Settings.

This design makes the saved provider configuration and its discovered models the single source of truth. User-visible models must come from the provider API; no preset or placeholder model may be presented as if it were discovered.

## Scope

The change covers four behaviors:

1. Add an explicit custom provider option using the OpenAI-compatible protocol.
2. Discover real models using the entered Base URL and API key.
3. Restrict connection testing and default-model selection to successfully discovered models.
4. Make Assistant V2 consume the same saved providers and models so a valid configuration is usable without a second setup path.

Other provider protocols and unrelated Assistant V2 runtime behavior are outside this change.

## User Experience

### Custom provider

The preset list includes a localized Custom Provider entry. Selecting it exposes editable provider name, Base URL, optional website URL, and API keys. It uses the OpenAI-compatible model-list and chat-completions endpoints.

The user must provide a non-empty provider name, valid Base URL, and at least one API key before model discovery can run.

### Model discovery and selection

The dialog starts without a selected model. The user requests model discovery after entering connection details. During discovery, the UI shows a loading state. On success, it renders the returned model IDs in a selector and chooses the first model only when no valid selection exists.

Changing the Base URL or API key invalidates the previous discovery result, selected test model, and successful test result. This prevents stale models from being used with different credentials.

An empty discovery response is a real empty state, not an invitation to use a hard-coded fallback. Errors are classified before display.

### Connection test

Connection testing is disabled until discovery has returned at least one model and the user has selected one of those models. The test request always uses the selected discovered model. Arbitrary model text and hard-coded test models are not accepted by the UI flow.

Saving requires a selected discovered default model. The saved configuration includes the discovered model set and default model needed by Assistant V2.

### Assistant availability

Assistant V2 reads the saved provider records and their models through its provider-list contract. It is available when at least one saved provider has a usable key and at least one saved discovered model. It selects the saved default model when valid, otherwise the first saved discovered model.

The page distinguishes these states:

- Assistant runtime unavailable: the Assistant V2 bridge or daemon is not connected.
- No provider: no saved provider with a usable key exists.
- No model: providers exist but none has a saved discovered model.
- Ready: a provider and discovered model can be selected.

Configuration failures do not masquerade as runtime unavailability.

## Architecture and Data Flow

The renderer continues to call only `window.nativesAPI`. Provider credentials remain encrypted and are never returned to the renderer after saving.

The flow is:

1. The add-provider dialog sends Base URL, raw API key, and provider protocol through the provider adapter to discover models.
2. The backend normalizes, filters, sorts, and deduplicates the provider response.
3. The dialog selects and tests one model from that exact response.
4. Saving persists provider metadata, encrypted keys, discovered models, and the default model in one backend operation or transactionally equivalent sequence.
5. Assistant V2's provider-list response is derived from the saved provider/model records rather than a separate preset list.
6. The model selector and conversation creation use only models returned by that response.

If persistence requires a schema change, it must use migration logic with WAL and foreign-key requirements preserved. Provider deletion must also remove its persisted models according to the existing ownership rules.

## Component Boundaries

- `AddProviderDialog`: manages form state, discovery state, discovered-model selection, invalidation, and test eligibility.
- Provider adapter/API types: expose typed discovery, test, and save inputs without leaking secrets.
- Tauri provider commands: validate inputs, call provider adapters, encrypt credentials, and persist provider/model data.
- Assistant provider bridge: maps persisted provider data into the Assistant V2 `provider.list` contract.
- `AssistantWorkbench` and model selector: render runtime/configuration states and select only available provider models.

Shared model-selection rules should live in a small testable helper instead of being duplicated across components.

## Validation and Error Handling

- Model discovery rejects an empty or invalid Base URL and missing API key.
- Discovery results with empty IDs are removed and duplicate IDs are collapsed.
- A test request is rejected when the model is empty or not present in the current discovery result at the UI boundary.
- A saved default model must belong to the saved discovered-model set.
- Changing connection fields invalidates model and test state.
- All asynchronous UI paths expose loading, error, and success states.
- All new user-facing strings use synchronized Chinese and English i18n keys.

## Testing Strategy

Tests are written before production changes and cover:

1. Custom Provider appears and produces OpenAI-compatible configuration data.
2. Discovery results become the only selectable default/test models.
3. Changing Base URL or API key clears discovered models and prior test success.
4. Connection testing cannot run before discovery or with a model outside the discovered set.
5. Provider persistence round-trips discovered models and default model.
6. Assistant provider listing reflects saved providers/models and selects a valid default.
7. Assistant availability distinguishes runtime unavailable, no provider, no model, and ready states.
8. Chinese and English translation key structures remain synchronized.

Verification includes focused frontend/Rust tests, TypeScript type checking, relevant Cargo tests, and lint where supported by the repository.

## Acceptance Criteria

- The add-provider dialog offers a localized Custom Provider choice.
- No hard-coded model ID is used for provider testing or shown as a discovered model.
- The model used for testing is visibly selected from the latest successful discovery response.
- Saving is impossible until a discovered model is selected and successfully associated with the provider configuration.
- A saved valid provider appears in Assistant V2 with its discovered models without duplicate configuration.
- The Assistant page no longer reports generic unavailability for provider/model configuration problems.
- Existing encrypted credential handling and renderer-to-backend boundaries remain intact.
- Focused tests, type checking, and relevant Rust verification pass.
