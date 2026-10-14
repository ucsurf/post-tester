import { useState } from "react";
import { questionStore, type Question } from "../state/dialogs";
import { useStore } from "../state/store";
import { Modal } from "./Modal";

/** Renders the pending `confirmDialog` / `promptDialog`, if any. */
export function QuestionDialog() {
  const question = useStore(questionStore, (s) => s.question);
  if (!question) return null;
  return question.type === "confirm" ? <Confirm question={question} /> : <Prompt key={question.title} question={question} />;
}

function Confirm({ question }: { question: Extract<Question, { type: "confirm" }> }) {
  return (
    <Modal
      title={question.title}
      width={440}
      onClose={() => question.resolve(false)}
      footer={
        <>
          <button type="button" className="btn" onClick={() => question.resolve(false)}>
            Cancel
          </button>
          <button
            type="button"
            className={`btn ${question.danger ? "btn-danger btn-solid" : "btn-primary"}`}
            onClick={() => question.resolve(true)}
            data-autofocus
          >
            {question.confirmLabel}
          </button>
        </>
      }
    >
      <p>{question.message}</p>
    </Modal>
  );
}

function Prompt({ question }: { question: Extract<Question, { type: "prompt" }> }) {
  const [value, setValue] = useState(question.initial);
  const [touched, setTouched] = useState(false);
  const error = question.validate(value);

  const submit = () => {
    setTouched(true);
    if (!error) question.resolve(value);
  };

  return (
    <Modal
      title={question.title}
      width={460}
      onClose={() => question.resolve(null)}
      footer={
        <>
          <button type="button" className="btn" onClick={() => question.resolve(null)}>
            Cancel
          </button>
          <button type="submit" form="question-form" className="btn btn-primary">
            {question.confirmLabel}
          </button>
        </>
      }
    >
      <form
        id="question-form"
        className="stack"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        {question.message && <p className="muted">{question.message}</p>}
        <label className="field">
          <span>{question.label}</span>
          <input
            className="input input-mono"
            value={value}
            placeholder={question.placeholder}
            onChange={(e) => setValue(e.target.value)}
            aria-invalid={touched && Boolean(error)}
            data-autofocus
          />
        </label>
        {touched && error && <p className="tone-danger hint">{error}</p>}
      </form>
    </Modal>
  );
}
