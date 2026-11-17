import { useState, type FormEvent } from "react";
import { Check, Crosshair, Plus } from "lucide-react";
import type { Investigation } from "./api";

type InvestigationManagerProps = {
  investigations: Investigation[];
  activeInvestigationId: string | null;
  addressesByInvestigation: ReadonlyMap<string, number>;
  loading: boolean;
  saving: boolean;
  error: string;
  onSelect: (investigationId: string) => void;
  onCreate: (name: string) => Promise<void>;
  onRetry: () => void;
};

export function InvestigationManager({
  investigations,
  activeInvestigationId,
  addressesByInvestigation,
  loading,
  saving,
  error,
  onSelect,
  onCreate,
  onRetry,
}: InvestigationManagerProps) {
  const [name, setName] = useState("");
  const [validationError, setValidationError] = useState("");
  const [submitError, setSubmitError] = useState("");

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedName = name.trim();
    if (!trimmedName) {
      setValidationError("Enter an investigation name.");
      return;
    }
    if (trimmedName.length > 120) {
      setValidationError("Use 120 characters or fewer.");
      return;
    }

    setValidationError("");
    setSubmitError("");
    try {
      await onCreate(trimmedName);
      setName("");
    } catch {
      setSubmitError("Could not create the investigation. Try again.");
    }
  };

  return (
    <section
      className="investigation-manager"
      aria-labelledby="investigation-manager-title"
    >
      <header className="investigation-manager-heading">
        <span className="investigation-manager-mark" aria-hidden="true">
          <Crosshair size={18} />
        </span>
        <div>
          <h1 id="investigation-manager-title">Investigations</h1>
          <p>
            Create separate workspaces for saved addresses and graph snapshots.
          </p>
        </div>
      </header>

      <form
        className="investigation-create"
        onSubmit={(event) => void submit(event)}
      >
        <label htmlFor="investigation-name">New investigation</label>
        <div className="investigation-create-row">
          <input
            id="investigation-name"
            name="name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            maxLength={120}
            placeholder="For example: Treasury review"
            aria-invalid={Boolean(validationError || submitError)}
            aria-describedby={
              validationError || submitError
                ? "investigation-form-error"
                : undefined
            }
          />
          <button
            className="button button-primary"
            type="submit"
            disabled={saving}
          >
            <Plus size={16} /> {saving ? "Creating…" : "Create investigation"}
          </button>
        </div>
        {(validationError || submitError) && (
          <p
            className="investigation-form-error"
            id="investigation-form-error"
            role="alert"
          >
            {validationError || submitError}
          </p>
        )}
      </form>

      {error && (
        <div className="address-state address-error" role="alert">
          <span>{error}</span>
          <button className="text-action" type="button" onClick={onRetry}>
            Retry
          </button>
        </div>
      )}

      {loading ? (
        <p className="address-state" role="status">
          Loading investigations…
        </p>
      ) : investigations.length === 0 ? (
        <div className="flow-graph-empty investigation-empty">
          <div>
            <strong>No investigations yet</strong>
            <p>
              Create one to keep addresses, history, and saved graph snapshots
              together.
            </p>
          </div>
        </div>
      ) : (
        <ul className="investigation-list" aria-label="Saved investigations">
          {investigations.map((investigation) => {
            const isActive = investigation.id === activeInvestigationId;
            const addressCount =
              addressesByInvestigation.get(investigation.id) ?? 0;
            return (
              <li key={investigation.id}>
                <article
                  className={`investigation-card ${isActive ? "is-active" : ""}`}
                >
                  <div className="investigation-card-main">
                    <div className="investigation-card-title">
                      <strong>{investigation.name}</strong>
                      {isActive && <span>Active</span>}
                    </div>
                    <p>
                      {addressCount}{" "}
                      {addressCount === 1 ? "address" : "addresses"}
                      <span aria-hidden="true"> · </span>
                      Created{" "}
                      {new Date(investigation.created_at).toLocaleDateString()}
                    </p>
                  </div>
                  <button
                    className={
                      isActive
                        ? "button button-secondary"
                        : "button button-primary"
                    }
                    type="button"
                    onClick={() => onSelect(investigation.id)}
                    disabled={isActive}
                    aria-label={
                      isActive
                        ? `${investigation.name} is active`
                        : `Open ${investigation.name}`
                    }
                  >
                    {isActive ? <Check size={15} /> : null}
                    {isActive ? "Active" : "Open"}
                  </button>
                </article>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
