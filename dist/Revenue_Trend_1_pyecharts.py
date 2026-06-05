#!/usr/bin/env python3
"""
Auto-generated pyecharts chart: Revenue Trend
Source: sample_report.json
"""
import json, pathlib
from pyecharts.charts import Bar
from pyecharts import options as opts
from pyecharts.globals import ThemeType

BASE = pathlib.Path(__file__).resolve().parent
OUT_DIR = BASE / "dist"
OUT_DIR.mkdir(parents=True, exist_ok=True)

def create_chart():
    """生成图表，返回 Chart 对象"""
chart = Bar(init_opts=opts.InitOpts(theme=ThemeType.DARK))
chart.add_xaxis(["January", "February", "March"])
chart.add_yaxis("Product A", [320, 410, 380])
chart.add_yaxis("Product B", [240, 300, 420])
chart.add_yaxis("Product C", [180, 220, 290])
chart.set_global_opts(
    title_opts=opts.TitleOpts(title="Monthly Revenue (2026 Q1)"),
    tooltip_opts=opts.TooltipOpts(trigger="axis"),
    legend_opts=opts.LegendOpts(pos_left="center"),
)
    return chart

if __name__ == "__main__":
    chart = create_chart()
    # 渲染为HTML（可浏览器查看）
    html_path = OUT_DIR / "Revenue_Trend_pyecharts.html"
    chart.render(str(html_path))
    print(f"pyecharts HTML: {html_path}")
    
    # 也输出option JSON供比对
    opt_path = OUT_DIR / "Revenue_Trend_option.json"
    opt_path.write_text(
        json.dumps(chart.get_options(), ensure_ascii=False, indent=2),
        encoding="utf-8"
    )
    print(f"option JSON: {opt_path}")
