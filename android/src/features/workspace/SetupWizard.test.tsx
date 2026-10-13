import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SetupStepper } from "./SetupWizard";

describe("SetupStepper", () => {
  it("marks reached steps and keeps future steps locked", () => {
    const onStepChange = vi.fn();
    render(
      <SetupStepper
        phase="rules_confirmed"
        activeStep={3}
        mode="training"
        hasRuleIssues={false}
        onStepChange={onStepChange}
      />,
    );

    expect(
      screen.getByRole("button", { name: /赛制编辑与确认/ }),
    ).toHaveAttribute("aria-current", "step");
    fireEvent.click(screen.getByRole("button", { name: /辩题与材料/ }));
    expect(onStepChange).toHaveBeenCalledWith(1);
    expect(screen.getByRole("button", { name: /队伍与辩位/ })).toBeDisabled();
    expect(screen.getByText("训练模式无需测速")).toBeInTheDocument();
  });
});
