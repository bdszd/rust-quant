import { useEffect, useMemo, useState } from "react";
import {
  Activity, ArrowDownRight, ArrowUpRight, BarChart3, CheckCircle2,
  Clock3, Database, GitCompareArrows, LayoutDashboard, Play, RefreshCw,
  TrendingUp, Wallet, XCircle
} from "lucide-react";

type Dataset = { name: string; rows: number };
type StrategyType = "sma_cross" | "buy_and_hold";
type TaskStatus = "queued" | "running" | "succeeded" | "failed";
type Snapshot = {
  date: string; cash: number; position: number; close: number; market_value: number;
  average_cost: number; unrealized_pnl: number; realized_pnl: number; equity: number;
  daily_return: number; drawdown: number;
};
type Trade = {
  order_id: number; date: string; symbol: string; side: "buy" | "sell";
  quantity: number; price: number; fee: number;
};
type BacktestResult = {
  id: string; dataset: string; strategy: string; short_window: number | null;
  long_window: number | null; initial_cash: number; final_equity: number;
  total_return: number; annualized_return: number; max_drawdown: number;
  annualized_volatility: number; sharpe_ratio: number; win_rate: number;
  profit_factor: number; trade_count: number; closed_trade_count: number;
  trades: Trade[]; history: Snapshot[];
};
type BacktestTask = {
  id: string; dataset: string; strategy: string; status: TaskStatus;
  short_window: number | null; long_window: number | null;
  result: BacktestResult | null; error: string | null;
  execution_duration_ms: number | null;
};
type Page = "overview" | "new" | "datasets" | "history" | "compare";

const money = (value: number) => new Intl.NumberFormat("zh-CN", {
  style: "currency", currency: "CNY", maximumFractionDigits: 2
}).format(value);
const pct = (value: number) => `${(value * 100).toFixed(2)}%`;
const wait = (ms: number) => new Promise(resolve => window.setTimeout(resolve, ms));

export default function App() {
  const [page, setPage] = useState<Page>("overview");
  const [datasets, setDatasets] = useState<Dataset[]>([]);
  const [history, setHistory] = useState<BacktestTask[]>([]);
  const [dataset, setDataset] = useState("");
  const [cash, setCash] = useState("100000");
  const [commission, setCommission] = useState("0.0003");
  const [slippage, setSlippage] = useState("0.0005");
  const [mode, setMode] = useState("next_open");
  const [strategy, setStrategy] = useState<StrategyType>("sma_cross");
  const [shortWindow, setShortWindow] = useState("5");
  const [longWindow, setLongWindow] = useState("20");
  const [task, setTask] = useState<BacktestTask | null>(null);
  const [result, setResult] = useState<BacktestResult | null>(null);
  const [compareIds, setCompareIds] = useState<string[]>([]);
  const [apiOnline, setApiOnline] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  async function refreshAll() {
    try {
      const health = await fetch("/api/health");
      if (!health.ok) throw new Error("后端服务不可用");
      setApiOnline(true);
      const [datasetResponse, historyResponse] = await Promise.all([
        fetch("/api/datasets"), fetch("/api/backtests")
      ]);
      if (!datasetResponse.ok || !historyResponse.ok) throw new Error("读取工作台数据失败");
      const items: Dataset[] = await datasetResponse.json();
      const tasks: BacktestTask[] = await historyResponse.json();
      setDatasets(items);
      setHistory(tasks);
      setDataset(current => items.some(item => item.name === current) ? current : items[0]?.name ?? "");
    } catch (e) {
      setApiOnline(false);
      setError(e instanceof Error ? e.message : "无法连接后端");
    }
  }

  useEffect(() => { void refreshAll(); }, []);

  async function openTask(item: BacktestTask) {
    setTask(item);
    setResult(item.result);
    setError(item.error ?? "");
    if (item.result) setPage("overview");
  }

  async function runBacktest(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!dataset) { setError("请先选择行情数据集"); return; }
    const initialCash = Number(cash);
    const commissionRate = Number(commission);
    const slippageRate = Number(slippage);
    if (!Number.isFinite(initialCash) || initialCash <= 0) { setError("初始资金必须大于 0"); return; }
    if (![commissionRate, slippageRate].every(v => Number.isFinite(v) && v >= 0 && v < 1)) {
      setError("手续费率和滑点率必须在 0 到 1 之间"); return;
    }
    if (strategy === "sma_cross") {
      const short = Number(shortWindow), long = Number(longWindow);
      if (!Number.isInteger(short) || !Number.isInteger(long) || short < 1 || long <= short) {
        setError("均线参数必须是正整数，且短周期小于长周期"); return;
      }
    }

    setLoading(true); setError(""); setResult(null); setTask(null);
    try {
      const response = await fetch("/api/backtests", {
        method: "POST", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          dataset, initial_cash: initialCash, commission_rate: commissionRate,
          slippage_rate: slippageRate, execution_mode: mode, strategy,
          short_window: strategy === "sma_cross" ? Number(shortWindow) : null,
          long_window: strategy === "sma_cross" ? Number(longWindow) : null
        })
      });
      const created: BacktestTask | { error: string } = await response.json();
      if (!response.ok) throw new Error("error" in created ? (created.error ?? "提交回测失败") : "提交回测失败");
      const taskId = (created as BacktestTask).id;
      setTask(created as BacktestTask);

      // POST 返回的是任务受理信息，不是回测结果。轮询持久化任务直到完成。
      for (let attempt = 0; attempt < 180; attempt++) {
        await wait(1000);
        const statusResponse = await fetch(`/api/backtests/${encodeURIComponent(taskId)}`);
        if (!statusResponse.ok) throw new Error("读取回测任务状态失败");
        const latest: BacktestTask = await statusResponse.json();
        setTask(latest);
        if (latest.status === "succeeded" && latest.result) {
          setResult(latest.result);
          await refreshAll();
          setPage("overview");
          return;
        }
        if (latest.status === "failed") throw new Error(latest.error ?? "回测执行失败");
      }
      throw new Error("回测仍在执行中，可在「回测历史」中查看任务状态");
    } catch (e) {
      setError(e instanceof Error ? e.message : "回测请求失败");
      await refreshAll();
    } finally {
      setLoading(false);
    }
  }

  const completed = history.filter(item => item.status === "succeeded" && item.result);
  const selectedDataset = datasets.find(item => item.name === dataset);
  const compareResults = useMemo(
    () => compareIds.map(id => history.find(item => item.id === id)?.result).filter((item): item is BacktestResult => Boolean(item)),
    [compareIds, history]
  );
  const comparisonRows: { label: string; value: (item: BacktestResult) => string }[] = [
    { label: "总收益率", value: item => pct(item.total_return) },
    { label: "年化收益率", value: item => pct(item.annualized_return) },
    { label: "最大回撤", value: item => pct(item.max_drawdown) },
    { label: "夏普比率", value: item => item.sharpe_ratio.toFixed(3) },
    { label: "胜率", value: item => pct(item.win_rate) },
    { label: "成交次数", value: item => String(item.trade_count) },
  ];

  const titles: Record<Page, string> = {
    overview: "研究概览", new: "创建回测", datasets: "数据集管理",
    history: "回测历史", compare: "策略对比"
  };

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand"><div className="brand-mark"><Activity size={22}/></div><div><b>RUST QUANT</b><small>RESEARCH WORKSPACE</small></div></div>
        <div className="nav-caption">WORKSPACE</div>
        <NavItem active={page === "overview"} icon={<LayoutDashboard size={17}/>} onClick={() => setPage("overview")}>研究概览</NavItem>
        <NavItem active={page === "new"} icon={<Play size={17}/>} onClick={() => setPage("new")}>创建回测</NavItem>
        <NavItem active={page === "datasets"} icon={<Database size={17}/>} onClick={() => setPage("datasets")}>数据集管理</NavItem>
        <NavItem active={page === "history"} icon={<Clock3 size={17}/>} onClick={() => setPage("history")}>回测历史</NavItem>
        <NavItem active={page === "compare"} icon={<GitCompareArrows size={17}/>} onClick={() => setPage("compare")}>策略对比</NavItem>
        <div className="sidebar-bottom"><span className={`status-dot ${apiOnline ? "online" : ""}`}/><div><b>{apiOnline ? "后端已连接" : "后端未连接"}</b><small>Axum · localhost:8000</small></div></div>
      </aside>

      <main className="main-area">
        <header className="topbar">
          <div><div className="eyebrow">QUANTITATIVE PLATFORM / V1</div><h1>{titles[page]}</h1><p>单股票日线研究与回测环境</p></div>
          <button className="button button-ghost" onClick={() => void refreshAll()}><RefreshCw size={15}/>刷新</button>
        </header>
        {error && <div className="alert"><XCircle size={17}/>{error}</div>}

        {page === "new" && (
          <section className="content-grid">
            <div className="panel form-panel">
              <PanelTitle eyebrow="BACKTEST CONFIGURATION" title="回测参数"/>
              <form onSubmit={runBacktest}>
                <Field label="行情数据集">
                  <select value={dataset} onChange={e => setDataset(e.target.value)} required>
                    <option value="">选择一个数据集</option>
                    {datasets.map(item => <option key={item.name} value={item.name}>{item.name}</option>)}
                  </select>
                </Field>
                <div className="dataset-hint"><Database size={14}/>{selectedDataset ? `包含 ${selectedDataset.rows.toLocaleString()} 条日线记录` : "先将清洗后的 bars.csv 放入数据目录"}</div>
                <Field label="初始资金"><input type="number" min="1" step="any" value={cash} onChange={e => setCash(e.target.value)}/></Field>
                <div className="two-col">
                  <Field label="手续费率"><input type="number" min="0" max="0.999999" step="any" value={commission} onChange={e => setCommission(e.target.value)}/><small>0.0003 = 0.03%</small></Field>
                  <Field label="滑点率"><input type="number" min="0" max="0.999999" step="any" value={slippage} onChange={e => setSlippage(e.target.value)}/><small>0.0005 = 0.05%</small></Field>
                </div>
                <Field label="模拟成交时机">
                  <div className="segmented">
                    <button type="button" className={mode === "next_open" ? "chosen" : ""} onClick={() => setMode("next_open")}>次日开盘</button>
                    <button type="button" className={mode === "next_close" ? "chosen" : ""} onClick={() => setMode("next_close")}>次日收盘</button>
                  </div>
                </Field>
                <Field label="交易策略">
                  <select value={strategy} onChange={e => setStrategy(e.target.value as StrategyType)}>
                    <option value="sma_cross">SMA 双均线交叉</option><option value="buy_and_hold">买入并持有</option>
                  </select>
                </Field>
                {strategy === "sma_cross" && <div className="two-col">
                  <Field label="短周期"><input type="number" min="1" step="1" value={shortWindow} onChange={e => setShortWindow(e.target.value)}/></Field>
                  <Field label="长周期"><input type="number" min="2" step="1" value={longWindow} onChange={e => setLongWindow(e.target.value)}/></Field>
                </div>}
                <div className="strategy-description">{strategy === "sma_cross" ? "短期均线上穿长期均线时买入，下穿时卖出。信号在当前交易日收盘后生成，按所选成交模式在下一交易日执行。" : "首次行情产生买入信号，后续持续持有，不主动卖出。"}</div>
                <button className="button button-primary submit-button" type="submit" disabled={loading || !apiOnline || !dataset}><Play size={16}/>{loading ? "回测执行中…" : "提交并运行回测"}</button>
                {task && <div className="task-state"><span className={`state-badge ${task.status}`}>{task.status}</span><span>{task.id}</span></div>}
              </form>
            </div>
            <div className="panel guidance-panel"><div className="illustration"><BarChart3 size={34}/></div><h2>从行情到回测报告</h2><p>选择一个数据集并设置资金、费用和策略参数。任务会进入后端队列，完成后自动展示结果。</p><div className="flow"><span>01 参数配置</span><i/><span>02 Rust 引擎</span><i/><span>03 绩效报告</span></div><div className="note-box">当前 V1 聚焦单股票日线回测。成交价格、手续费、滑点和每日账户估值均由回测核心统一计算。</div></div>
          </section>
        )}

        {page === "datasets" && <section className="panel">
          <PanelTitle eyebrow="MARKET DATA" title="本地行情数据集"/>
          <p className="muted-copy">后端扫描清洗后的数据目录。每个数据集包含一只股票在指定日期范围内的日线 Bar。</p>
          {datasets.length === 0 ? <Empty title="还没有可用数据集" text="请检查 data/cleaned 目录以及对应的 bars.csv 和 quality_report.json。"/> :
            <div className="data-grid">{datasets.map(item => <div className="dataset-card" key={item.name}><div className="dataset-icon"><Database size={18}/></div><div><b>{item.name.split("/")[0]}</b><small>{item.name.split("/")[1]}</small></div><strong>{item.rows.toLocaleString()} <small>bars</small></strong><button className="button button-ghost" onClick={() => {setDataset(item.name);setPage("new");}}>用于回测</button></div>)}</div>}
        </section>}

        {page === "history" && <section className="panel">
          <PanelTitle eyebrow="PERSISTED TASKS" title="最近回测任务"/>
          {history.length === 0 ? <Empty title="暂无回测记录" text="运行第一次回测后，任务会保存在 PostgreSQL 中。"/> :
            <div className="table-wrap"><table><thead><tr><th>任务 ID</th><th>数据集</th><th>策略</th><th>状态</th><th>收益率</th><th>执行耗时</th><th/></tr></thead><tbody>
              {history.map(item => <tr key={item.id}><td className="mono">{item.id}</td><td>{item.dataset}</td><td>{strategyName(item.strategy)}</td><td><span className={`state-badge ${item.status}`}>{item.status}</span></td><td className={item.result && item.result.total_return >= 0 ? "positive" : "negative"}>{item.result ? pct(item.result.total_return) : "—"}</td><td>{item.execution_duration_ms == null ? "—" : `${item.execution_duration_ms} ms`}</td><td><button className="text-button" onClick={() => void openTask(item)}>查看</button></td></tr>)}
            </tbody></table></div>}
        </section>}

        {page === "compare" && <section className="panel">
          <PanelTitle eyebrow="PERFORMANCE COMPARISON" title="策略与参数对比"/>
          <p className="muted-copy">选择已完成的回测任务，横向比较收益和风险指标。</p>
          <div className="compare-picker">{completed.map(item => <label key={item.id}><input type="checkbox" checked={compareIds.includes(item.id)} onChange={e => setCompareIds(current => e.target.checked ? [...current, item.id].slice(-4) : current.filter(id => id !== item.id))}/><span>{strategyName(item.strategy)} · {item.dataset} · {item.id.slice(-8)}</span></label>)}</div>
          {compareResults.length > 0 ? <div className="table-wrap"><table><thead><tr><th>指标</th>{compareResults.map(r => <th key={r.id}>{strategyName(r.strategy)}<small className="table-sub">{r.id.slice(-8)}</small></th>)}</tr></thead><tbody>
            {comparisonRows.map(row => <tr key={row.label}><td>{row.label}</td>{compareResults.map(r => <td key={r.id}>{row.value(r)}</td>)}</tr>)}
          </tbody></table></div> : <Empty title="选择回测结果" text="最多选择 4 次已完成的回测进行比较。"/>}
        </section>}

        {page === "overview" && <>
          {!result ? <section className="overview-grid">
            <div className="panel welcome-panel"><span className="kicker">YOUR RESEARCH SPACE</span><h2>把策略想法变成可验证的回测</h2><p>从本地 A 股日线数据开始，配置策略并运行 Rust 回测引擎。所有结果都会保存，方便后续比较。</p><button className="button button-primary" onClick={() => setPage("new")}><Play size={16}/>创建第一次回测</button></div>
            <div className="panel quick-stats"><div><span>数据集</span><b>{datasets.length}</b></div><div><span>已完成回测</span><b>{completed.length}</b></div><div><span>任务总数</span><b>{history.length}</b></div></div>
            <div className="panel recent-panel"><PanelTitle eyebrow="RECENT ACTIVITY" title="最近回测"/>{history.slice(0,5).length ? history.slice(0,5).map(item => <button className="recent-row" key={item.id} onClick={() => void openTask(item)}><span><b>{strategyName(item.strategy)}</b><small>{item.dataset} · {item.id.slice(-8)}</small></span><span className={`state-badge ${item.status}`}>{item.status}</span></button>) : <Empty title="暂无任务" text="创建回测后，最近任务会显示在这里。"/>}</div>
          </section> : <ResultView result={result} task={task} onBack={() => setPage("history")}/>}
        </>}

        <footer>RUST QUANT · BACKTEST FRAMEWORK V1</footer>
      </main>
    </div>
  );
}

function strategyName(name: string) { return name === "sma_cross" ? "SMA 双均线" : name === "buy_and_hold" ? "买入持有" : name; }
function NavItem({active,icon,children,onClick}:{active:boolean;icon:React.ReactNode;children:React.ReactNode;onClick:()=>void}) {
  return <button className={`nav-item ${active ? "active" : ""}`} onClick={onClick}>{icon}{children}</button>;
}
function Field({label,children}:{label:string;children:React.ReactNode}) { return <label className="field"><span>{label}</span>{children}</label>; }
function PanelTitle({eyebrow,title}:{eyebrow:string;title:string}) { return <div className="panel-title"><span className="kicker">{eyebrow}</span><h2>{title}</h2></div>; }
function Empty({title,text}:{title:string;text:string}) { return <div className="empty-state"><div className="empty-icon"><BarChart3 size={24}/></div><b>{title}</b><p>{text}</p></div>; }

function ResultView({result,task,onBack}:{result:BacktestResult;task:BacktestTask|null;onBack:()=>void}) {
  const points = result.history;
  return <section className="report">
    <div className="report-heading"><div><span className="kicker">BACKTEST REPORT</span><h2>{strategyName(result.strategy)} <small>{result.dataset}</small></h2><p className="muted-copy">任务 {result.id} · {points[0]?.date ?? "—"} 至 {points[points.length-1]?.date ?? "—"} · {task?.execution_duration_ms ?? "—"} ms</p></div><button className="button button-ghost" onClick={onBack}>查看历史</button></div>
    <div className="metric-grid">
      <Metric label="最终资产" value={money(result.final_equity)} detail={`初始资金 ${money(result.initial_cash)}`} icon={<Wallet size={17}/>}/>
      <Metric label="总收益率" value={pct(result.total_return)} detail="相对初始资金" icon={result.total_return >= 0 ? <ArrowUpRight size={17}/> : <ArrowDownRight size={17}/>} positive={result.total_return >= 0}/>
      <Metric label="年化收益率" value={pct(result.annualized_return)} detail="按 252 个交易日折算" icon={<TrendingUp size={17}/>}/>
      <Metric label="最大回撤" value={pct(result.max_drawdown)} detail="净值峰值至后续低点" icon={<ArrowDownRight size={17}/>}/>
      <Metric label="夏普比率" value={result.sharpe_ratio.toFixed(3)} detail="无风险利率按 0 处理" icon={<Activity size={17}/>}/>
      <Metric label="年化波动率" value={pct(result.annualized_volatility)} detail="日收益率标准差年化" icon={<Activity size={17}/>}/>
      <Metric label="胜率" value={pct(result.win_rate)} detail={`${result.closed_trade_count} 笔已平仓交易`} icon={<CheckCircle2 size={17}/>}/>
      <Metric label="成交次数" value={String(result.trade_count)} detail={`盈亏比 ${result.profit_factor >= 999 ? "∞" : result.profit_factor.toFixed(2)}`} icon={<BarChart3 size={17}/>}/>
    </div>
    <div className="panel chart-panel"><PanelTitle eyebrow="EQUITY CURVE" title="账户净值曲线"/><LineChart values={points.map(p => p.equity)} dates={points.map(p => p.date)} format={money}/></div>
    <div className="panel chart-panel"><PanelTitle eyebrow="DRAWDOWN" title="回撤曲线"/><LineChart values={points.map(p => -p.drawdown * 100)} dates={points.map(p => p.date)} format={v => `${v.toFixed(2)}%`} zeroBased/></div>
    <div className="panel table-panel"><PanelTitle eyebrow="EXECUTIONS" title="成交记录"/>{result.trades.length ? <div className="table-wrap"><table><thead><tr><th>日期</th><th>代码</th><th>方向</th><th>数量</th><th>成交价</th><th>手续费</th></tr></thead><tbody>{result.trades.map(t => <tr key={t.order_id}><td>{t.date}</td><td>{t.symbol}</td><td className={t.side === "buy" ? "positive" : "negative"}>{t.side === "buy" ? "买入" : "卖出"}</td><td>{t.quantity}</td><td>{money(t.price)}</td><td>{money(t.fee)}</td></tr>)}</tbody></table></div> : <p className="muted-copy">本次回测没有产生成交。</p>}</div>
    <div className="panel table-panel"><PanelTitle eyebrow="DAILY PORTFOLIO" title="每日账户快照"/><div className="table-wrap history-table"><table><thead><tr><th>日期</th><th>收盘价</th><th>持仓</th><th>平均成本</th><th>现金</th><th>市值</th><th>账户资产</th><th>当日收益</th><th>回撤</th></tr></thead><tbody>{points.map(p => <tr key={p.date}><td>{p.date}</td><td>{money(p.close)}</td><td>{p.position}</td><td>{p.average_cost ? money(p.average_cost) : "—"}</td><td>{money(p.cash)}</td><td>{money(p.market_value)}</td><td>{money(p.equity)}</td><td>{pct(p.daily_return)}</td><td>{pct(p.drawdown)}</td></tr>)}</tbody></table></div></div>
  </section>;
}

function Metric({label,value,detail,icon,positive}:{label:string;value:string;detail:string;icon:React.ReactNode;positive?:boolean}) {
  return <div className="metric-card"><div className="metric-top"><span>{label}</span><i>{icon}</i></div><strong className={positive === undefined ? "" : positive ? "positive" : "negative"}>{value}</strong><small>{detail}</small></div>;
}
function LineChart({values,dates,format,zeroBased=false}:{values:number[];dates:string[];format:(n:number)=>string;zeroBased?:boolean}) {
  if (values.length < 2) return <Empty title="数据点不足" text="至少需要两个交易日才能绘制曲线。"/>;
  const min = zeroBased ? Math.min(0,...values) : Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;
  const coords = values.map((v,i) => `${(i/(values.length-1))*100},${90-((v-min)/range)*80}`);
  const path = coords.join(" ");
  return <><div className="chart-labels"><span>{format(max)}</span><span>{format(min)}</span></div><svg className="line-chart" viewBox="0 0 100 100" preserveAspectRatio="none"><line x1="0" y1="10" x2="100" y2="10"/><line x1="0" y1="50" x2="100" y2="50"/><line x1="0" y1="90" x2="100" y2="90"/><polyline points={path}/></svg><div className="chart-dates"><span>{dates[0]}</span><span>{dates[dates.length-1]}</span></div></>;
}
