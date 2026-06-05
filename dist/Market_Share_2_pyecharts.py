#!/usr/bin/env python3
"""
Auto-generated pyecharts chart: Market Share
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
chart.add_yaxis("Market Share", [{"value": 45, "name": "Product A"}, {"value": 28, "name": "Product B"}, {"value": 18, "name": "Product C"}, {"value": 9, "name": "Others"}])
chart.set_global_opts(
    title_opts=opts.TitleOpts(title="Market Share by Product"),
    tooltip_opts=opts.TooltipOpts(trigger="axis"),
    legend_opts=opts.LegendOpts(pos_left="center"),
)
    return chart

if __name__ == "__main__":
    chart = create_chart()
    # 渲染为HTML（可浏览器查看）
    html_path = OUT_DIR / "Market_Share_pyecharts.html"
    chart.render(str(html_path))
    print(f"pyecharts HTML: {html_path}")
    
    # 也输出option JSON供比对
    opt_path = OUT_DIR / "Market_Share_option.json"
    opt_path.write_text(
        json.dumps(chart.get_options(), ensure_ascii=False, indent=2),
        encoding="utf-8"
    )
    print(f"option JSON: {opt_path}")
