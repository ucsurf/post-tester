import { Icon } from "../../ui/Icon";
import "./activity.css";

type TestLike = { name: string; passed: boolean; detail?: string | null | undefined };

/** Pass/fail list of assertions with failure details. */
export function TestList({ tests, emptyText }: { tests: readonly TestLike[]; emptyText: string }) {
  if (tests.length === 0) return <p className="muted pad">{emptyText}</p>;
  const failed = tests.filter((t) => !t.passed).length;
  return (
    <div className="tests">
      <p className={`tests-summary ${failed ? "tone-danger" : "tone-success"}`}>
        {failed ? `${failed} of ${tests.length} failed` : `All ${tests.length} passed`}
      </p>
      <ul>
        {tests.map((t, i) => (
          <li key={`${t.name}-${i}`} className={t.passed ? "pass" : "fail"}>
            <span className={t.passed ? "tone-success" : "tone-danger"}>
              <Icon name={t.passed ? "check" : "x"} size={12} />
              <span className="sr-only">{t.passed ? "Passed" : "Failed"}</span>
            </span>
            <div className="grow">
              <code>{t.name}</code>
              {t.detail && <p className="test-detail">{t.detail}</p>}
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
