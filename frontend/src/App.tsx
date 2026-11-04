
import { useEffect, useState } from "react";
import {
  Activity,
  ArrowDownRight,
  ArrowUpRight,
  BarChart3,
  Database,
  Play,
  RefreshCw,
  Wallet,
} from "lucide-react";

type Dataset = {
  name: string;
  rows: number;
};

type StrategyType = "sma_cross" | "buy_and_hold";

type Snapshot = {
  date: string;
  cash: number;
  position: number;
  close: number;
  equity: number;
};

type Trade = {
  order_id: number;
  date: string;
  symbol: string;
  side: "buy" | "sell";
  quantity: number;
  price: number;
  fee: number;
};

type BacktestResult = {
  id: string;
  dataset: string;
  initial_cash: number;
  final_equity: number;
  total_return: number;
  max_drawdown: number;
  trades: Trade[];
  history: Snapshot[];
};

const money = (n: number) =>
  new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    maximumFractionDigits: 2,
  }).format(n);

const percent = (n: number) => `${(n * 100).toFixed(2)}%`;

export default function App() {
  const [datasets, setDatasets] = useState<Dataset[]>([]);
  const [dataset, setDataset] = useState("");
  const [cash, setCash] = useState("100000");
  const [commission, setCommission] = useState("0.001");
  const [slippage, setSlippage] = useState("0.0005");
  const [mode, setMode] = useState("next_open");

  // 新增：当前选择的策略和 SMA 参数
  const [strategy, setStrategy] = useState<StrategyType>("sma_cross");
  const [shortWindow, setShortWindow] = useState("2");
  const [longWindow, setLongWindow] = useState("3");

  const [result, setResult] = useState<BacktestResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [apiOnline, setApiOnline] = useState(false);
  const [error, setError] = useState("");

  async function loadDatasets() {
    try {
      setError("");

      const healthResponse = await fetch("/api/health");
      if (!healthResponse.ok) {
        throw new Error("后端服务不可用");
      }
      setApiOnline(true);

      const response = await fetch("/api/datasets");
      if (!response.ok) {
        throw new Error("无法读取数据集列表");
      }

      const items: Dataset[] = await response.json();
      setDatasets(items);

      setDataset((current) =>
        items.some((item) => item.name === current)
          ? current
          : items[0]?.name ?? ""
      );
    } catch (e) {
      setApiOnline(false);
      setError(e instanceof Error ? e.message : "无法连接后端");
    }
  }

  useEffect(() => {
    void loadDatasets();
  }, []);

  async function runBacktest(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();

    // 前端先做一次校验，后端还会再次校验
    if (strategy === "sma_cross") {
      const short = Number(shortWindow);
      const long = Number(longWindow);

      if (
        !Number.isInteger(short) ||
        !Number.isInteger(long) ||
        short < 1 ||
        long < 1 ||
        short >= long
      ) {
        setError("SMA 参数无效：短周期和长周期必须是正整数，且短周期小于长周期。");
        return;
      }
    }

    setLoading(true);
    setError("");
    setResult(null);

    try {
      const response = await fetch("/api/backtests", {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          dataset,
          initial_cash: Number(cash),
          commission_rate: Number(commission),
          slippage_rate: Number(slippage),
          execution_mode: mode,

          // 新增：将策略配置发送给后端
          strategy,
          short_window: Number(shortWindow),
          long_window: Number(longWindow),
        }),
      });

      const payload = await response.json();

      if (!response.ok) {
        throw new Error(payload.error ?? "回测失败");
      }

      setResult(payload as BacktestResult);
    } catch (e) {
      setError(e instanceof Error ? e.message : "回测请求失败");
    } finally {
      setLoading(false);
    }
  }

  const points = result?.history ?? [];
  const equityValues = points.map((point) => point.equity);
  const minEquity = equityValues.length > 0
    ? Math.min(...equityValues)
    : 0;
  const maxEquity = equityValues.length > 0
    ? Math.max(...equityValues)
    : 0;
  const range = maxEquity - minEquity || 1;

  const chartPoints = points
    .map((point, index) => {
      const x =
        points.length <= 1
          ? 50
          : (index / (points.length - 1)) * 100;

      const y = 90 - ((point.equity - minEquity) / range) * 80;
      return `${x},${y}`;
    })
    .join(" ");

  const selectedDataset = datasets.find((item) => item.name === dataset);

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-icon">
            <Activity size={23} />
          </div>
          <div>
            <strong>RUST QUANT</strong>
            <small>RESEARCH WORKSPACE</small>
          </div>
        </div>

        <div className="nav-label">WORKSPACE</div>
        <div className="nav-item active">
          <BarChart3 size={18} /> 回测工作台
        </div>
        <div className="nav-item">
          <Database size={18} /> 本地数据集
        </div>

        <div className="connection">
          <span className={`dot ${apiOnline ? "online" : ""}`} />
          <div>
            <strong>{apiOnline ? "后端已连接" : "等待后端连接"}</strong>
            <small>Axum · localhost:8000</small>
          </div>
        </div>
      </aside>

      <main className="main">
        <header className="topbar">
          <div>
            <div className="eyebrow">
              QUANTITATIVE PLATFORM / BACKTEST
            </div>
            <h1>回测工作台</h1>
            <p>配置回测参数，运行 Rust 引擎，查看账户表现。</p>
          </div>

          <button
            className="refresh"
            type="button"
            onClick={() => void loadDatasets()}
          >
            <RefreshCw size={16} /> 刷新数据
          </button>
        </header>

        {error && <div className="error-banner">{error}</div>}

        <div className="layout">
          <section className="panel config">
            <div className="section-heading">
              <div>
                <span className="kicker">CONFIGURATION</span>
                <h2>回测配置</h2>
              </div>
              <span className="tag">01 / SETUP</span>
            </div>

            <form onSubmit={runBacktest}>
              <label htmlFor="dataset">行情数据集</label>
              <select
                id="dataset"
                value={dataset}
                onChange={(event) => setDataset(event.target.value)}
                required
              >
                {datasets.length === 0 && (
                  <option value="">暂无 CSV 数据集</option>
                )}
                {datasets.map((item) => (
                  <option key={item.name} value={item.name}>
                    {item.name}
                  </option>
                ))}
              </select>

              <div className="hint">
                <Database size={14} />
                {selectedDataset
                  ? `${selectedDataset.rows} 条行情记录`
                  : "将 CSV 文件放入项目 data/ 目录"}
              </div>

              <label htmlFor="cash">初始资金</label>
              <div className="money-input">
                <span>$</span>
                <input
                  id="cash"
                  type="number"
                  min="1"
                  step="any"
                  value={cash}
                  onChange={(event) => setCash(event.target.value)}
                  required
                />
              </div>

              <div className="two-columns">
                <div>
                  <label htmlFor="commission">手续费率</label>
                  <input
                    id="commission"
                    type="number"
                    min="0"
                    max="0.999999"
                    step="any"
                    value={commission}
                    onChange={(event) => setCommission(event.target.value)}
                    required
                  />
                  <small>0.001 = 0.1%</small>
                </div>
                <div>
                  <label htmlFor="slippage">滑点率</label>
                  <input
                    id="slippage"
                    type="number"
                    min="0"
                    max="0.999999"
                    step="any"
                    value={slippage}
                    onChange={(event) => setSlippage(event.target.value)}
                    required
                  />
                  <small>0.0005 = 0.05%</small>
                </div>
              </div>

              <label>模拟成交时机</label>
              <div className="mode-options">
                <button
                  type="button"
                  className={mode === "next_open" ? "mode selected" : "mode"}
                  onClick={() => setMode("next_open")}
                >
                  <strong>次日开盘</strong>
                  <small>Next Open</small>
                </button>
                <button
                  type="button"
                  className={mode === "next_close" ? "mode selected" : "mode"}
                  onClick={() => setMode("next_close")}
                >
                  <strong>次日收盘</strong>
                  <small>Next Close</small>
                </button>
              </div>

              {/* 新增：策略选择 */}
              <label htmlFor="strategy">回测策略</label>
              <select
                id="strategy"
                value={strategy}
                onChange={(event) =>
                  setStrategy(event.target.value as StrategyType)
                }
              >
                <option value="sma_cross">SMA 交叉策略</option>
                <option value="buy_and_hold">买入持有策略</option>
              </select>

              {strategy === "sma_cross" ? (
                <>
                  <div className="two-columns">
                    <div>
                      <label htmlFor="short-window">短周期</label>
                      <input
                        id="short-window"
                        type="number"
                        min="1"
                        step="1"
                        value={shortWindow}
                        onChange={(event) =>
                          setShortWindow(event.target.value)
                        }
                        required
                      />
                      <small>用于计算短期均线</small>
                    </div>
                    <div>
                      <label htmlFor="long-window">长周期</label>
                      <input
                        id="long-window"
                        type="number"
                        min="2"
                        step="1"
                        value={longWindow}
                        onChange={(event) =>
                          setLongWindow(event.target.value)
                        }
                        required
                      />
                      <small>必须大于短周期</small>
                    </div>
                  </div>

                  <div className="strategy-note">
                    <span className="demo-tag">SMA</span>
                    <div>
                      <strong>
                        短期均线上穿买入，下穿卖出
                      </strong>
                      <p>
                        当前参数：短周期 {shortWindow || "—"}，长周期{" "}
                        {longWindow || "—"}。
                      </p>
                    </div>
                  </div>
                </>
              ) : (
                <div className="strategy-note">
                  <span className="demo-tag">B&H</span>
                  <div>
                    <strong>买入持有</strong>
                    <p>
                      策略首次产生买入信号，之后保持持仓，不主动卖出。
                    </p>
                  </div>
                </div>
              )}

              <button
                className="run-button"
                type="submit"
                disabled={loading || !dataset || !apiOnline}
              >
                <Play size={16} fill="currentColor" />
                {loading ? "正在运行回测…" : "运行回测"}
              </button>

              <p className="footnote">
                本地执行 · 数据不需要上传到外部服务
              </p>
            </form>
          </section>

          <section className="results">
            {!result && !loading && (
              <div className="panel empty">
                <div className="empty-icon">
                  <BarChart3 size={28} />
                </div>
                <h2>等待回测结果</h2>
                <p>
                  选择一个数据集，然后点击「运行回测」。
                  完成后这里会展示收益指标、账户曲线和成交记录。
                </p>
                <div className="steps">
                  <span>01 配置参数</span>
                  <span>02 调用 Rust 引擎</span>
                  <span>03 查看结果</span>
                </div>
              </div>
            )}

            {loading && (
              <div className="panel empty">
                <RefreshCw className="loading-icon" size={28} />
                <h2>正在执行回测</h2>
                <p>后端正在读取行情并调用 backtest-core。</p>
              </div>
            )}

            {result && (
              <>
                <div className="result-heading">
                  <div>
                    <span className="kicker">BACKTEST REPORT</span>
                    <h2>回测结果</h2>
                  </div>
                  <span className="tag">{result.id}</span>
                </div>

                <div className="metrics">
                  <Metric
                    label="最终资产"
                    value={money(result.final_equity)}
                    detail={`初始资金 ${money(result.initial_cash)}`}
                    icon={<Wallet size={17} />}
                  />
                  <Metric
                    label="总收益率"
                    value={percent(result.total_return)}
                    detail="相对初始资金"
                    icon={
                      result.total_return >= 0
                        ? <ArrowUpRight size={17} />
                        : <ArrowDownRight size={17} />
                    }
                    positive={result.total_return >= 0}
                  />
                  <Metric
                    label="最大回撤"
                    value={percent(result.max_drawdown)}
                    detail="历史峰值至后续低点"
                    icon={<ArrowDownRight size={17} />}
                  />
                  <Metric
                    label="成交次数"
                    value={String(result.trades.length)}
                    detail={`${result.history.length} 个交易日`}
                    icon={<Activity size={17} />}
                  />
                </div>

                <div className="panel chart-panel">
                  <div className="section-heading">
                    <div>
                      <span className="kicker">EQUITY CURVE</span>
                      <h2>账户资产变化</h2>
                    </div>
                    <span className="legend">每日收盘估值</span>
                  </div>

                  {points.length > 1 ? (
                    <>
                      <div className="chart-value">
                        <span>{money(maxEquity)}</span>
                        <span>{money(minEquity)}</span>
                      </div>
                      <svg
                        className="chart"
                        viewBox="0 0 100 100"
                        preserveAspectRatio="none"
                      >
                        <line x1="0" y1="10" x2="100" y2="10" />
                        <line x1="0" y1="50" x2="100" y2="50" />
                        <line x1="0" y1="90" x2="100" y2="90" />
                        <polyline points={chartPoints} />
                      </svg>
                      <div className="chart-dates">
                        <span>{points[0]?.date}</span>
                        <span>{points[points.length - 1]?.date}</span>
                      </div>
                    </>
                  ) : (
                    <p className="hint">历史数据点不足，无法绘制曲线。</p>
                  )}
                </div>

                <div className="panel table-panel">
                  <div className="section-heading">
                    <div>
                      <span className="kicker">EXECUTIONS</span>
                      <h2>成交记录</h2>
                    </div>
                    <span className="tag">
                      {result.trades.length} FILLS
                    </span>
                  </div>

                  {result.trades.length === 0 ? (
                    <p className="hint">本次回测没有产生成交。</p>
                  ) : (
                    <div className="table-scroll">
                      <table>
                        <thead>
                          <tr>
                            <th>日期</th>
                            <th>方向</th>
                            <th>数量</th>
                            <th>成交价</th>
                            <th>手续费</th>
                          </tr>
                        </thead>
                        <tbody>
                          {result.trades.map((trade) => (
                            <tr key={trade.order_id}>
                              <td>{trade.date}</td>
                              <td>
                                <span className={`side ${trade.side}`}>
                                  {trade.side === "buy" ? "买入" : "卖出"}
                                </span>
                              </td>
                              <td>{trade.quantity}</td>
                              <td>{money(trade.price)}</td>
                              <td>{money(trade.fee)}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                </div>

                <div className="panel table-panel">
                  <div className="section-heading">
                    <div>
                      <span className="kicker">DAILY HISTORY</span>
                      <h2>每日账户快照</h2>
                    </div>
                    <span className="tag">
                      {result.history.length} DAYS
                    </span>
                  </div>

                  <div className="table-scroll">
                    <table>
                      <thead>
                        <tr>
                          <th>日期</th>
                          <th>收盘价</th>
                          <th>持仓</th>
                          <th>现金</th>
                          <th>账户资产</th>
                        </tr>
                      </thead>
                      <tbody>
                        {result.history.map((snapshot) => (
                          <tr key={snapshot.date}>
                            <td>{snapshot.date}</td>
                            <td>{money(snapshot.close)}</td>
                            <td>{snapshot.position}</td>
                            <td>{money(snapshot.cash)}</td>
                            <td>{money(snapshot.equity)}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </div>
              </>
            )}
          </section>
        </div>

        <footer>RUST QUANT · LOCAL BACKTEST MVP · v0.1.0</footer>
      </main>
    </div>
  );
}

function Metric({
  label,
  value,
  detail,
  icon,
  positive,
}: {
  label: string;
  value: string;
  detail: string;
  icon: React.ReactNode;
  positive?: boolean;
}) {
  return (
    <div className="metric">
      <div className="metric-top">
        <span>{label}</span>
        <span className="metric-icon">{icon}</span>
      </div>
      <strong
        className={
          positive === undefined ? "" : positive ? "positive" : "negative"
        }
      >
        {value}
      </strong>
      <small>{detail}</small>
    </div>
  );
}
