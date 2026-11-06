"""Build the CS452 midterm status report PDF in the style of the proposal.

    .venv/bin/pip install reportlab
    .venv/bin/python docs/report/build_midterm.py
"""

from pathlib import Path

from reportlab.lib import colors
from reportlab.lib.enums import TA_CENTER, TA_JUSTIFY
from reportlab.lib.pagesizes import A4
from reportlab.lib.styles import ParagraphStyle
from reportlab.lib.units import mm
from reportlab.platypus import (
    HRFlowable,
    Image,
    KeepTogether,
    ListFlowable,
    ListItem,
    Paragraph,
    SimpleDocTemplate,
    Spacer,
    Table,
    TableStyle,
)

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "docs/report/CS452_Midterm_Sazeides_Yiakoumis.pdf"
FIG = ROOT / "results/sim/plots/sweep.png"

SERIF, BOLD, ITALIC, MONO = "Times-Roman", "Times-Bold", "Times-Italic", "Courier"
BODY = 9.6

base = ParagraphStyle("body", fontName=SERIF, fontSize=BODY, leading=BODY * 1.22,
                      alignment=TA_JUSTIFY, spaceAfter=3)
small = ParagraphStyle("small", parent=base, fontSize=8.6, leading=10.4, spaceAfter=0)
cell = ParagraphStyle("cell", parent=base, fontSize=8.6, leading=10.2, alignment=0, spaceAfter=0)
cellc = ParagraphStyle("cellc", parent=cell, alignment=TA_CENTER)
center = ParagraphStyle("center", parent=base, alignment=TA_CENTER, spaceAfter=0)
title = ParagraphStyle("title", parent=center, fontName=BOLD, fontSize=17, leading=20, spaceAfter=4)
kicker = ParagraphStyle("kicker", parent=center, fontSize=8.8, letterSpacing=0.6, spaceAfter=4)
head = ParagraphStyle("head", parent=base, fontName=BOLD, fontSize=10.5, spaceBefore=6,
                      spaceAfter=2, alignment=0)
caption = ParagraphStyle("caption", parent=small, alignment=TA_JUSTIFY, spaceBefore=2, spaceAfter=4)


def smallcaps(text, big=10.5, little=8.3):
    out = []
    for word in text.split(" "):
        out.append(f'<font size="{big}">{word[0]}</font><font size="{little}">{word[1:].upper()}</font>')
    return " ".join(out)


def section(n, name):
    return Paragraph(f"{n}. {smallcaps(name)}", head)


def p(text, style=base):
    return Paragraph(text, style)


def code(text):
    return f'<font face="{MONO}" size="{BODY - 1}">{text}</font>'


def numbered(items):
    return ListFlowable(
        [ListItem(p(t), leftIndent=14, value=i + 1) for i, t in enumerate(items)],
        bulletType="1", bulletFormat="%s.", leftIndent=14, bulletFontName=SERIF,
        bulletFontSize=BODY, spaceAfter=2,
    )


def bullets(items):
    return ListFlowable(
        [ListItem(p(t), leftIndent=12) for t in items],
        bulletType="bullet", start="•", leftIndent=12, bulletFontSize=BODY, spaceAfter=2,
    )


def booktabs(rows, widths, numeric_from=None):
    """A table with LaTeX booktabs-style rules and a bold header row."""
    data = []
    for r, row in enumerate(rows):
        out = []
        for c, v in enumerate(row):
            st = cellc if numeric_from is not None and c >= numeric_from else cell
            out.append(Paragraph(f"<b>{v}</b>" if r == 0 else v, st))
        data.append(out)
    t = Table(data, colWidths=widths, hAlign="CENTER")
    t.setStyle(TableStyle([
        ("LINEABOVE", (0, 0), (-1, 0), 1.0, colors.black),
        ("LINEBELOW", (0, 0), (-1, 0), 0.5, colors.black),
        ("LINEBELOW", (0, -1), (-1, -1), 1.0, colors.black),
        ("VALIGN", (0, 0), (-1, -1), "TOP"),
        ("TOPPADDING", (0, 0), (-1, -1), 1.6),
        ("BOTTOMPADDING", (0, 0), (-1, -1), 1.6),
        ("LEFTPADDING", (0, 0), (-1, -1), 3),
        ("RIGHTPADDING", (0, 0), (-1, -1), 3),
    ]))
    return t


def build():
    doc = SimpleDocTemplate(str(OUT), pagesize=A4, leftMargin=19 * mm, rightMargin=19 * mm,
                            topMargin=16 * mm, bottomMargin=15 * mm,
                            title="scx_slo: Midterm Status Report",
                            author="Thrasos Sazeides, Markos Yiakoumis")
    W = doc.width
    s = []

    # Header, as in the proposal.
    s += [
        p("CS452 DATACENTER COMPUTING · MIDTERM STATUS REPORT", kicker),
        p("scx_slo: SLO-Aware CPU Scheduling for Co-located<br/>Datacenter Workloads with sched_ext", title),
        p(f'Thrasos Sazeides&nbsp;&nbsp;{code("tsazei01@ucy.ac.cy")}&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;'
          f'Markos Yiakoumis&nbsp;&nbsp;{code("mgiako02@ucy.ac.cy")}', center),
        p("Department of Computer Science, University of Cyprus · November 8, 2026 · "
          f'{code("github.com/vcTaz/scx_slo")}', center),
        Spacer(1, 4),
        HRFlowable(width="100%", thickness=1.1, color=colors.black, spaceBefore=0, spaceAfter=3),
    ]

    # 1. Summary
    s += [
        section(1, "Summary"),
        p("The proposal planned baseline measurements and a first working scx_slo for week 9. "
          "<b>We could not run any experiment on CloudLab</b>: the bare-metal nodes our profile needs "
          f'({code("xl170")}, with {code("c220g5")} as the alternative) were not available to us, so '
          "the testbed was never instantiated. There are therefore <b>no measured results in this report</b>. "
          "We used the time to build and test everything that does not need the testbed: the feedback "
          "controller, the latency windows, a closed-loop simulator, a first partial-mode scx_slo "
          "scheduler, and a fully scripted testbed and benchmark harness that runs end to end once nodes "
          "are available. In simulation the controller meets the proposal's success criterion; "
          "this has yet to be confirmed on hardware."),
    ]

    # 2. Progress
    s += [
        section(2, "Progress Since the Proposal"),
        numbered([
            f"<b>Feedback controller</b> ({code('crates/slo-ctrl')}). Each 100 ms tick it reads p99 over a "
            "short (0.3 s) and a long (2 s) window and sets two levers: <i>k</i>, the number of cores batch "
            "tasks may use, and their time slice. It cuts fast (halve <i>k</i> and slice when short-window "
            "p99 exceeds 90% of the SLO), holds for 0.5 s, then recovers slowly (one core, then 2 ms of slice, "
            "every 0.3 s while long-window p99 is under 70% of the SLO). Missing data means hold.",
            "<b>Latency windows.</b> Log-linear histograms (8 sub-buckets per power of two, at most 12.5% "
            "overestimate of p99) kept as running sums, so a window update costs O(1) and the same code can "
            "read a BPF histogram map.",
            "<b>Tests.</b> 26 unit and synthetic-trace tests (load step, burst, slow drift, missing data, "
            "hold-down, limits), all passing.",
            f"<b>Simulator</b> ({code('crates/slo-sim')}). Runs the real controller against a model of "
            "memcached (one M/M/1 queue whose service rate drops as batch tasks get more cores and longer "
            "slices) and reproduces our planned baselines (Section 3).",
            f"<b>scx_slo scheduler, first version</b> ({code('scheduler/')}). Built on scx_rustland_core "
            f"(scx v1.1.4). It runs in partial mode, so only batch tasks started through a small "
            f"{code('ext_exec')} launcher are scheduled by sched_ext while memcached stays on EEVDF. It confines "
            f"batch tasks to a CPU set ({code('--cpus')}) with a fixed slice, and logs dispatch statistics every "
            "second. The controller is not yet wired in, and the scheduler has not been run, because it needs a "
            "kernel with sched_ext.",
            f"<b>Testbed and harness.</b> A CloudLab profile (2 nodes, Ubuntu 24.04, 100 GB blockstore), "
            "setup scripts for both nodes (sched_ext kernel, scx, memtier_benchmark 2.5.2, batch inputs), a "
            "pre-flight check, and benchmark scripts for every baseline: memcached alone, EEVDF, a static "
            f"cgroup ({code('AllowedCPUs')}), stock scx_rustland, scx_slo on all CPUs, and scx_slo with a fixed "
            "core limit. One command runs a load sweep and a parser summarises the results.",
        ]),
    ]

    # 3. Simulation results
    rows = [
        ["Variant (simulated)", "p99 at 30%", "p99 at 50%", "p99 at 70%", "p99 at 90%",
         "Within SLO, 10–70%", "Batch thr., 10–70%"],
        ["(b) EEVDF, no control", "328 µs", "655 µs", "92 ms", "94 ms", "4 of 7 loads", "100%"],
        ["(c) Static limit, sized for 90%", "147 µs", "180 µs", "262 µs", "459 µs",
         "7 of 7", "12%"],
        ["Fixed half of the cores", "180 µs", "262 µs", "459 µs", "2.4 ms", "7 of 7", "50%"],
        ["<b>scx_slo (adaptive)</b>", "328 µs", "387 µs", "393 µs", "393 µs",
         "<b>7 of 7</b>", "<b>91%</b>"],
    ]
    cw = [W * 0.27] + [W * 0.095] * 4 + [W * 0.175, W * 0.175]
    s += [
        section(3, "Simulation Results"),
        p("Because the testbed was unavailable, we evaluated the controller in simulation with a placeholder "
          "SLO of 500 µs on a 16-core machine (load as a percentage of memcached's peak; 5 runs per point). "
          "<b>These numbers come from a model, not from measurements</b>; only the shape of the trade-off is "
          "meaningful."),
        booktabs(rows, cw, numeric_from=1),
        Spacer(1, 4),
        KeepTogether([
            Image(str(FIG), width=W * 0.86, height=W * 0.86 * 668 / 1764),
            p("<b>Figure 1.</b> Simulated load sweep. Left: memcached p99 (log scale) against the 500 µs "
              "SLO. Right: batch throughput relative to running unrestricted. scx_slo stays under the SLO at "
              "every load while giving batch jobs most of the machine up to 60% load.", caption),
        ]),
        p("In a load burst (30% to 85% for 40 s), scx_slo exceeded the SLO for 3 ticks (0.3 s) and was back "
          "under it within 0.5 s, while EEVDF stayed over it for the whole burst. The simulation also exposed "
          "four tuning problems that we will address on hardware:"),
        bullets([
            "The first tick of a burst is always lost, because the controller acts one tick after it observes.",
            "The hysteresis band stops recovery early: at 40% load batch jobs settle at 80% throughput instead of 100%.",
            "It over-cuts at high load: at 90% load batch throughput falls to 5%, below the static limit's 12%.",
            "At 50% load the controller oscillates between cutting and recovering.",
        ]),
    ]

    # 4. Problems and risks
    s += [
        section(4, "Problems and Risks"),
        bullets([
            "<b>Testbed (blocking).</b> No CloudLab nodes were available, so no baseline is measured. The "
            "scripts are written and checked, but have never run on real nodes and will likely need small fixes. "
            "If CloudLab is still unavailable by 12 November, we will ask the instructor for an allocation and "
            "fall back to any machine where we can boot a Linux 6.12+ kernel with sched_ext (a lab machine, or "
            "a VM for functional tests only, since a VM is too noisy for tail latency).",
            "<b>The SLO is a placeholder.</b> It will be set from the measured p99 of memcached alone, and the "
            "simulation re-run with it.",
            "<b>EEVDF may already protect memcached.</b> In partial mode the fair class runs ahead of sched_ext, "
            "so memcached may already be protected without any controller. The first hardware test compares "
            "scx_slo on all cores with memcached alone to find out.",
            "<b>Latency probe.</b> The BPF probe that measures memcached's p99 in the kernel is not built yet. "
            "Fallback: the load generator reports p99 to the scheduler over a socket.",
        ]),
    ]

    # 5. Revised timeline
    tl = [
        ["Weeks", "Milestone"],
        ["10 (9–15 Nov)", "Testbed running (or fallback machine); baselines (a)–(d) measured; real SLO "
         "chosen; latency probe working, or switch to the socket fallback by 12 Nov"],
        ["11 (16–22 Nov)", "Controller wired into scx_slo through a BPF map (core set and slice); "
         "first closed-loop runs on hardware"],
        ["12 (23–29 Nov)", "Tuning based on the four simulation findings; full load sweeps with "
         "5 repetitions; burst reaction time and scheduler overhead"],
        ["13 (30 Nov–4 Dec)", "<b>Final presentation</b>"],
    ]
    s += [
        KeepTogether([section(5, "Revised Timeline"),
                      p("The proposal's weeks 7–9 slipped by about one week because of the testbed. "
                        "Simulation results already de-risk the controller design, so we compress the remaining work:"),
                      booktabs(tl, [W * 0.17, W * 0.83])]),
    ]

    # 6. Team
    s += [
        section(6, "Team Contributions"),
        p("As planned in the proposal, we share every part of the project equally: both of us wrote code for "
          "the controller, simulator and testbed scripts, each reviewed the other's changes, and we co-wrote "
          "this report."),
        p(smallcaps("References"), head),
        p("[1] sched_ext schedulers and tools. https://github.com/sched-ext/scx", small),
        p("[2] scx_slo source code, simulation data and scripts. https://github.com/vcTaz/scx_slo", small),
    ]

    doc.build(s)
    print(OUT)


if __name__ == "__main__":
    build()
