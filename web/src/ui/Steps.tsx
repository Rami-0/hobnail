import { CheckIcon } from "lucide-react";

/** A step indicator for a multi-step dialog: done steps get a check,
 * the current one is named for assistive technology. */
export function Steps({ steps, current }: { steps: string[]; current: number }) {
  return <ol className="steps" aria-label={`Step ${current + 1} of ${steps.length}`}>
    {steps.map((label, index) => <li key={label} className="step" data-state={index < current ? "done" : index === current ? "current" : "todo"} aria-current={index === current ? "step" : undefined}>
      <span className="step-mark" aria-hidden="true">{index < current ? <CheckIcon /> : index + 1}</span><span className="step-label">{label}</span>
    </li>)}
  </ol>;
}
