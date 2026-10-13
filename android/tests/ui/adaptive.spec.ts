import { test, expect } from "@playwright/test";

for (const viewport of [
  { width: 360, height: 800 },
  { width: 412, height: 915 },
  { width: 768, height: 1024 },
  { width: 1024, height: 768 },
  { width: 1280, height: 800 },
  { width: 915, height: 412 },
]) {
  for (const mode of ["speech", "cross", "free"]) {
    test(`${viewport.width}×${viewport.height} ${mode}: 页面不溢出且仍能访问八个辩位`, async ({
      page,
    }) => {
      await page.setViewportSize(viewport);
      await page.goto(`/tests/ui/index.html?mode=${mode}`);
      await expect(
        page.getByRole("region", { name: "赛场", exact: true }),
      ).toBeVisible();
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= window.innerWidth,
        ),
      ).toBe(true);
      const expanded = viewport.width >= 840 && viewport.height >= 480;
      if (expanded)
        await expect(
          page.getByLabel("双方辩席", { exact: true }),
        ).toBeVisible();
      else
        await expect(page.getByLabel("双方辩席", { exact: true })).toBeHidden();
      await page.getByRole("button", { name: "打开双方辩位" }).click();
      const panel = page.getByRole("dialog", { name: "双方辩位" });
      await expect(panel.locator(".seat")).toHaveCount(8);
      await expect(
        panel.getByRole("button", { name: /反方一辩/ }),
      ).toBeDisabled();
      await page.getByRole("button", { name: "关闭双方辩位" }).click();
      const tooSmall = await page
        .locator("button:visible")
        .evaluateAll((buttons) =>
          buttons
            .filter((button) => button.getBoundingClientRect().height < 43.5)
            .map((button) => button.textContent),
        );
      expect(tooSmall).toEqual([]);
      if (mode === "speech" && [360, 768, 1024].includes(viewport.width)) {
        await page.screenshot({
          path: `docs/acceptance/layout-${viewport.width}x${viewport.height}.png`,
          fullPage: true,
        });
      }
    });
  }
}

test("旋转、来回切换人工与 AI、模型等待期间回看历史均不重置草稿或推进比赛", async ({
  page,
}) => {
  await page.setViewportSize({ width: 360, height: 800 });
  await page.goto("/tests/ui/index.html");
  const input = page.getByRole("textbox", { name: "本环节回答" });
  await input.fill("我正在编辑，旋转与接管后不能消失。");
  await page.getByRole("button", { name: "交给 AI", exact: true }).click();
  await expect(input).toHaveValue("我正在编辑，旋转与接管后不能消失。");
  await page.getByRole("button", { name: "让模型完成本轮" }).click();
  const log = page.getByRole("log");
  await log.evaluate((node) => {
    node.scrollTop = 0;
    node.dispatchEvent(new Event("scroll"));
  });
  await expect(page.getByLabel("跟随最新")).not.toBeChecked();
  await expect(page.getByRole("button", { name: "取消生成" })).toBeEnabled();
  await page.setViewportSize({ width: 1024, height: 768 });
  await expect(input).toHaveValue("我正在编辑，旋转与接管后不能消失。");
  await page.getByRole("button", { name: "我来发言", exact: true }).click();
  await expect(input).toBeEnabled();
  await expect(input).toHaveValue("我正在编辑，旋转与接管后不能消失。");
  await expect(page.getByLabel("合成测试提交次数")).toHaveText("0");
});
