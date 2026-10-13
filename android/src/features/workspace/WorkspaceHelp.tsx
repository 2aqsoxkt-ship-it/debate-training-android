import { Drawer } from "../../components/ui/Overlay";
import "./WorkspaceHelp.css";

export function WorkspaceHelp({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  return (
    <Drawer open={open} title="使用说明" onClose={onClose}>
      <div className="workspace-help">
        <p className="workspace-help-intro">
          先准备，再确认，最后进入独立赛场。查看说明不会离开当前步骤。
        </p>
        <section aria-labelledby="help-setup">
          <h3 id="help-setup">赛前准备</h3>
          <ol>
            <li>建立或选用档案，选择模式、阵营和 AI 难度。</li>
            <li>填写辩题与参考材料；材料放背景、证据和来源，赛制单独填写。</li>
            <li>
              使用四对四预设或识别自定义赛制。识别失败会保留原文，仍可手动添加、删除和调整环节；解决阻断问题后确认规则。
            </li>
            <li>
              拆题并核对双方方向、八位辩手任务。修改辩题、材料、模式、阵营或难度后，需要重新拆题并确认阵容。
            </li>
            <li>选择人工辩位，核对赛程，确认阵容后正式开始。</li>
          </ol>
        </section>
        <section aria-labelledby="help-mode">
          <h3 id="help-mode">训练与评分</h3>
          <p>
            训练模式可控制任意一人、多人，也可全 AI 模拟。评分模式只能选择同一方
            1–4 个辩位，并完成两轮各 60 秒打字校准；粘贴不能作为有效测速。
          </p>
          <p>
            速度只校正输入与编辑成本，不直接加分。逆向编辑、超限和有效发言参与评价。
          </p>
        </section>
        <section aria-labelledby="help-words">
          <h3 id="help-words">字数与团队额度</h3>
          <dl className="workspace-help-budgets">
            <div>
              <dt>正反方主额度</dt>
              <dd>各 4320 字</dd>
            </div>
            <div>
              <dt>正反方自由辩</dt>
              <dd>各 960 字，全队共享</dd>
            </div>
          </dl>
          <p>
            上述为默认换算值，可在赛制编辑器修改。新比赛按有效字数推进，思考、打字等待和逐字展示不消耗比赛额度；旧时间比赛保留原规则。
          </p>
        </section>
        <section aria-labelledby="help-interaction">
          <h3 id="help-interaction">质询与自由辩</h3>
          <p>
            质询由合法候选辩位参与，AI
            可以决定派谁提问和回答；人工插入时选择自己的提问辩位。只有质询方能按赛制打断回答，被质询方不能打断或反问。默认问题和回答均扣提问方主额度。
          </p>
          <p>
            自由辩是两队对话，不固定一对一。正方先发，此后轮流；持权方可选择本方任意辩位，也可由
            AI 派人。自由辩不能打断，一方额度用尽后另一方可继续或放弃余额。
          </p>
        </section>
        <section aria-labelledby="help-recovery">
          <h3 id="help-recovery">查看记录与失败恢复</h3>
          <p>
            AI
            等待时仍可滚动实录。上翻查看历史会暂停跟随，点击“回到最新”恢复。失败先看提示，重试不会把同一条已确认发言重复扣字；质询状态不确定时先同步现场。
          </p>
          <p>
            本次打开期间，准备草稿在前往模型设置后返回时保留。未提交内容不是磁盘存档，不保证关闭软件后恢复。历史恢复以实际保存的数据为准，旧记录缺少策略或任务时需重新拆题。
          </p>
        </section>
        <section aria-labelledby="help-privacy">
          <h3 id="help-privacy">本机数据与模型连接</h3>
          <p>
            比赛记录保存在本机；API Key 由 Android Keystore
            加密保护。未验证连接的本地模拟不能代表真实 AI
            能力；余额不足时充值后重新测试连接。模型设置不会自动发起整场比赛。
          </p>
        </section>
      </div>
    </Drawer>
  );
}
