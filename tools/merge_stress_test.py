# -*- coding: utf-8 -*-
"""语义归并压力对照实验：同一份 ~5000 字 prompt，只改 num_ctx / num_predict。
用法: python merge_stress_test.py --tag A --num_ctx 4096 --num_predict -1
"""
import argparse, json, time, urllib.request

HOST = "http://127.0.0.1:11434/api/generate"

NAMES = """钻采院安全检查报告、钻采院安全检查记录表、安全检查隐患整改台账、安全生产检查通报、
钻井工艺方案、钻井工程设计说明书、钻井日报、钻井复杂情况分析、固井施工方案、完井报告、
压裂设计、酸化施工总结、试油方案、修井作业计划、井下工具清单、采油工艺优化方案、
智能工厂平台需求说明、智能工厂平台总体设计、智能工厂平台接口文档、智能工厂平台测试报告、
智能工厂平台培训材料、智能工厂平台验收单、工业互联网标识解析方案、数据中台建设方案、
边缘计算网关选型、PLC程序说明、SCADA组态备份说明、DCS点表、仪表选型手册、控制柜接线图、
费率清单、服务报价表、年度框架协议、供应商准入资料、招标文件、投标文件、评标报告、
合同评审记录、采购订单、验收结算单、发票扫描件、付款申请单、预算编制说明、成本核算表、
部门月度总结、部门年度总结、周例会纪要、专题会议纪要、领导讲话稿、工作汇报PPT、
人员花名册、岗位职责说明、绩效考核办法、培训签到表、继续教育学院结业证书、
职称评审材料、入职登记表、劳动合同、社保缴纳明细、考勤统计表、
项目立项申请、项目可行性研究、项目进度计划、项目里程碑清单、项目验收报告、项目复盘总结、
质量管理体系文件、内审检查表、不合格品处理单、纠正预防措施、质量月报、
科研项目申报书、专利申请书、技术论文、标准规范汇编、技术交底记录、技术评审意见、
信息化建设规划、网络安全应急预案、等保测评报告、服务器巡检记录、数据库备份策略、
固定资产台账、设备维修记录、车辆使用登记、办公用品领用单、印章使用审批单、
党员发展材料、工会活动方案、廉政风险防控清单、意识形态工作责任制、
科技成果鉴定材料、技术奖励申报、技师工作室计划、技能竞赛方案、
环境监测报告、排污许可材料、能耗统计分析、碳排放核算表、应急预案演练记录、
客户拜访记录、客户满意度调查、售后服务单、投诉处理记录、市场分析报告、
产品说明书、产品检测报告、样机测试记录、工艺变更通知、图纸会审记录""".replace("\n", "").split("、")

PROMPT = (
    "任务：把下面这 %d 个真实文件夹名做语义归并，合成不超过 12 个归档类别。\n"
    "规则：\n"
    "1. 只输出 JSON，不要解释。格式：{\"categories\":[{\"name\":\"类别名\",\"members\":[\"原名\",...]}]}\n"
    "2. 每个原名必须且只能出现在一个类别里，不要漏、不要改名。\n"
    "3. 类别名用 2-4 个汉字，按业务实质归类，不要按文件后缀归类。\n\n"
    "文件夹名列表：\n%s"
) % (len(NAMES), "\n".join("%d. %s" % (i + 1, n) for i, n in enumerate(NAMES)))


def run(tag, num_ctx, num_predict, think):
    payload = {
        "model": "qwen3.5:9b",
        "prompt": PROMPT,
        "stream": True,
        "think": think,
        "options": {"temperature": 0.2, "num_ctx": num_ctx, "num_predict": num_predict},
    }
    print("=" * 60)
    print("[%s] prompt 字符数=%d  行数=%d  num_ctx=%s  num_predict=%s  think=%s"
          % (tag, len(PROMPT), PROMPT.count("\n"), num_ctx, num_predict, think))
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(HOST, data=data, headers={"Content-Type": "application/json"})
    t0 = time.time()
    ttft = None
    chunks, seen = [], 0
    try:
        with urllib.request.urlopen(req, timeout=600) as r:
            for line in r:
                if not line.strip():
                    continue
                obj = json.loads(line)
                if obj.get("response"):
                    if ttft is None:
                        ttft = time.time() - t0
                    chunks.append(obj["response"])
                    seen += 1
                if obj.get("done"):
                    el = time.time() - t0
                    txt = "".join(chunks)
                    print("[%s] 完成: 总耗时 %.1fs / 首字 %.1fs / 输出块 %d / 输出字符 %d"
                          % (tag, el, ttft or -1, seen, len(txt)))
                    print("[%s] done_reason=%s  prompt_eval=%s tok  eval=%s tok"
                          % (tag, obj.get("done_reason"),
                             obj.get("prompt_eval_count"), obj.get("eval_count")))
                    ev = obj.get("eval_count") or 0
                    if ev:
                        print("[%s] 生成速度 %.2f tok/s" % (tag, ev / max(el - (obj.get("prompt_eval_duration", 0) / 1e9), 0.01)))
                    ok = False
                    try:
                        s = txt.find("{"); e = txt.rfind("}")
                        j = json.loads(txt[s:e + 1])
                        cats = j.get("categories", [])
                        n = sum(len(c.get("members", [])) for c in cats)
                        print("[%s] JSON 可解析=是  类别数=%d  覆盖条目=%d/%d  %s"
                              % (tag, len(cats), n, len(NAMES), "完整" if n == len(NAMES) else "不完整"))
                        ok = True
                    except Exception as ex:
                        print("[%s] JSON 可解析=否 (%s)  原始尾部: %r" % (tag, ex, txt[-300:]))
                    return ok
    except Exception as ex:
        print("[%s] 异常/超时: %s  已收 %d 块 / 耗时 %.1fs" % (tag, ex, seen, time.time() - t0))
        print("[%s] 已收内容尾部: %r" % (tag, "".join(chunks)[-300:]))
        return False


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--tag", default="A")
    p.add_argument("--num_ctx", type=int, default=4096)
    p.add_argument("--num_predict", type=int, default=-1)
    p.add_argument("--think", default="false")
    a = p.parse_args()
    run(a.tag, a.num_ctx, a.num_predict, a.think.lower() == "true")
