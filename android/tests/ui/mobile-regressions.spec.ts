import { expect, test } from "@playwright/test";

for (const width of [360, 412, 768, 1024]) {
  for (const mode of ["review", "editor"]) {
    test(`${width}px ${mode} 赛前布局中文保持横向可读且编辑可用`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height: 915 });
      await page.goto(`/tests/ui/setup.html?mode=${mode}`);
      // Phone font scaling must not collapse any schedule column to one glyph.
      if (width < 600)
        await page.addStyleTag({ content: "html { font-size: 20px; }" });
      if (mode === "review") {
        const row = page.locator(".schedule-preview tbody tr").first();
        const stage = row.locator("td").nth(1);
        expect((await stage.boundingBox())!.width).toBeGreaterThan(140);
        expect(
          (await row.locator("td").last().boundingBox())!.width,
        ).toBeGreaterThan(100);
        await page
          .getByRole("combobox", { name: "第1行辩位" })
          .selectOption("AFF_2");
        await expect(
          page.getByRole("combobox", { name: "第1行辩位" }),
        ).toHaveValue("AFF_2");
        await expect(page.getByLabel("人工赛程修改次数")).toHaveText("1");
        await expect(page.getByText(/全队共享/)).toBeVisible();
      } else {
        const heading = page.locator(".rule-stage-title").first();
        expect((await heading.boundingBox())!.width).toBeGreaterThan(140);
        await page.locator(".rule-stage-summary").first().click();
        await page
          .getByRole("textbox", { name: "环节名称", exact: true })
          .fill("修改后的立论陈词");
        await expect(heading).toContainText("修改后的立论陈词");
        await page.getByRole("button", { name: "保存赛程修改" }).click();
        await expect(page.getByLabel("人工赛程修改次数")).toHaveText("1");
      }
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
      if (width === 360)
        await page.screenshot({
          path: `docs/acceptance/mobile-${mode}-360.png`,
          fullPage: true,
        });
    });
  }
}

test("手机大字体下质询阶段名称不拆成逐字纵排", async ({ page }) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await page.goto("/tests/ui/index.html?mode=cross");
  await page.addStyleTag({ content: "html { font-size: 20px; }" });
  const lines = await page
    .locator(".cross-phase-line span")
    .evaluateAll((labels) =>
      labels.map((label) => {
        const range = document.createRange();
        range.selectNodeContents(label);
        return new Set(
          Array.from(range.getClientRects(), (rect) => Math.round(rect.top)),
        ).size;
      }),
    );
  expect(lines).toEqual([1, 1, 1, 1]);
});

test("浏览器真实 textarea 事件：候选词提交、重复回调、中间修改、粘贴均一致", async ({
  page,
}) => {
  await page.goto("/tests/ui/setup.html?mode=input");
  const input = page.getByRole("textbox", { name: "手机输入回放测试" });
  await input.evaluate((element) => {
    const el = element as HTMLTextAreaElement;
    el.dispatchEvent(
      new CompositionEvent("compositionstart", { bubbles: true }),
    );
    el.value = "甲乙丙";
    el.dispatchEvent(
      new InputEvent("input", {
        bubbles: true,
        isComposing: true,
        inputType: "insertCompositionText",
      }),
    );
    el.dispatchEvent(
      new CompositionEvent("compositionend", { bubbles: true, data: "甲乙丙" }),
    );
    el.dispatchEvent(
      new InputEvent("beforeinput", {
        bubbles: true,
        inputType: "insertText",
        data: "甲乙丙",
      }),
    );
    el.dispatchEvent(
      new InputEvent("input", { bubbles: true, inputType: "insertText" }),
    );
    el.value = "甲丁丙";
    el.dispatchEvent(
      new InputEvent("input", {
        bubbles: true,
        inputType: "insertReplacementText",
      }),
    );
    el.value = "甲丁粘贴丙";
    el.dispatchEvent(
      new InputEvent("input", { bubbles: true, inputType: "insertFromPaste" }),
    );
  });
  const proof = JSON.parse(await page.getByLabel("回放结果").innerText());
  expect(proof.text).toBe("甲丁粘贴丙");
  expect(proof.replayed).toBe(proof.text);
  expect(proof.claimed).toMatchObject({
    forwardTypedUnits: 4,
    reverseUnits: 1,
    reverseActions: 1,
    pasteUnits: 2,
  });
});
