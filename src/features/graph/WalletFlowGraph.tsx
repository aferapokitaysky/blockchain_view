import { useMemo, useState } from "react";
import type { WalletHistory } from "../investigations/api";
import { buildWalletFlowGraph, findObservedPaths } from "./model";
import "./WalletFlowGraph.css";

type WalletFlowGraphProps = {
  histories: WalletHistory[];
};

function shortAddress(address: string): string {
  return `${address.slice(0, 8)}…${address.slice(-6)}`;
}

export function WalletFlowGraph({ histories }: WalletFlowGraphProps) {
  const graph = useMemo(() => buildWalletFlowGraph(histories), [histories]);
  const [selectedNode, setSelectedNode] = useState("");
  const [maxDepth, setMaxDepth] = useState<1 | 2 | 3>(2);
  const focusedNode = graph.nodes.find((node) => node.id === selectedNode);
  const pathAnalysis = useMemo(
    () =>
      focusedNode
        ? findObservedPaths(graph.edges, focusedNode.id, maxDepth)
        : null,
    [focusedNode, graph.edges, maxDepth],
  );
  const nodesById = useMemo(
    () => new Map(graph.nodes.map((node) => [node.id, node])),
    [graph.nodes],
  );
  const edgesById = useMemo(
    () => new Map(graph.edges.map((edge) => [edge.id, edge])),
    [graph.edges],
  );
  const focusedEdges = focusedNode
    ? graph.edges.filter(
        (edge) => edge.from === focusedNode.id || edge.to === focusedNode.id,
      )
    : [];

  return (
    <section className="flow-graph-panel" aria-labelledby="flow-graph-title">
      <div className="activity-head flow-graph-head">
        <div>
          <h2 id="flow-graph-title">Wallet connections</h2>
          <p>
            Direct EVM transfers from saved observations. Bitcoin counterpart
            addresses are not available from address-level history.
          </p>
        </div>
        <div className="flow-graph-counts" aria-live="polite">
          <span>{graph.nodes.length} addresses</span>
          <span>{graph.edges.length} connections</span>
        </div>
      </div>

      <div className="flow-path-explorer">
        <div className="flow-path-controls">
          <label htmlFor="flow-path-depth">Maximum observed hops</label>
          <select
            id="flow-path-depth"
            value={maxDepth}
            onChange={(event) => {
              const value = event.target.value;
              setMaxDepth(value === "1" ? 1 : value === "3" ? 3 : 2);
            }}
          >
            <option value={1}>1 hop</option>
            <option value={2}>2 hops</option>
            <option value={3}>3 hops</option>
          </select>
          <span>
            {focusedNode && pathAnalysis
              ? `${pathAnalysis.paths.length} addresses reachable from ${focusedNode.label}`
              : "Select an address to trace observed outgoing paths"}
          </span>
        </div>
        <p className="flow-path-disclaimer">
          Paths connect observed sender-to-recipient events. They do not prove
          that the same asset or funds moved continuously between hops.
        </p>
      </div>

      {graph.edges.length === 0 ? (
        <div className="flow-graph-empty">
          <span className="flow-graph-empty-mark" aria-hidden="true">
            <svg viewBox="0 0 40 40">
              <circle cx="10" cy="20" r="4" />
              <circle cx="30" cy="10" r="4" />
              <circle cx="30" cy="30" r="4" />
              <path d="M14 18 26 12M14 22l12 6" />
            </svg>
          </span>
          <div>
            <strong>No direct links found</strong>
            <p>
              Sync EVM wallet history to reveal transfers between addresses.
              Bitcoin observations are not used to infer counterparties.
            </p>
          </div>
        </div>
      ) : (
        <>
          <div className="flow-graph-legend" aria-label="Graph legend">
            <span>
              <i className="tracked-dot" /> Saved address
            </span>
            <span>
              <i className="external-dot" /> Counterparty
            </span>
            <span>
              <i className="edge-dot" /> Observed transfer
            </span>
          </div>
          <div
            className="flow-graph-canvas"
            tabIndex={0}
            aria-label="Wallet transfer network, scroll to explore"
          >
            <svg
              viewBox="0 0 1000 500"
              role="img"
              aria-labelledby="flow-svg-title flow-svg-desc"
            >
              <title id="flow-svg-title">
                Observed wallet transfer network
              </title>
              <desc id="flow-svg-desc">
                {graph.nodes.length} addresses connected by {graph.edges.length}{" "}
                observed EVM transfer groups. Select an address for details.
              </desc>
              <defs>
                <marker
                  id="flow-arrow"
                  markerWidth="8"
                  markerHeight="8"
                  refX="7"
                  refY="4"
                  orient="auto"
                >
                  <path d="M0 0 8 4 0 8z" />
                </marker>
              </defs>
              <g className="flow-edges">
                {graph.edges.map((edge, index) => {
                  const from = nodesById.get(edge.from);
                  const to = nodesById.get(edge.to);
                  if (!from || !to) return null;
                  const offset = ((index % 5) - 2) * 16;
                  const midX = (from.x + to.x) / 2;
                  const midY = (from.y + to.y) / 2 + offset;
                  const isPathEdge = pathAnalysis?.pathEdgeIds.has(edge.id);
                  return (
                    <g key={edge.id}>
                      <title>
                        {`${shortAddress(from.address)} to ${shortAddress(to.address)}: ${edge.amount} ${edge.asset}, ${edge.transfers} event${edge.transfers === 1 ? "" : "s"}`}
                      </title>
                      <path
                        className={`flow-edge-line ${isPathEdge ? "is-path" : pathAnalysis ? "is-dimmed" : ""}`}
                        d={`M ${from.x} ${from.y} Q ${midX} ${midY} ${to.x} ${to.y}`}
                        markerEnd="url(#flow-arrow)"
                        strokeWidth={Math.min(
                          1.4 + Math.log2(edge.transfers + 1),
                          4,
                        )}
                      />
                    </g>
                  );
                })}
              </g>
              <g className="flow-nodes">
                {graph.nodes.map((node) => (
                  <g
                    key={node.id}
                    className={`flow-node ${node.tracked ? "is-tracked" : "is-external"} ${selectedNode === node.id ? "is-selected is-origin" : ""} ${pathAnalysis?.depthByNode.has(node.id) && node.id !== selectedNode ? "is-reachable" : ""} ${pathAnalysis && !pathAnalysis.depthByNode.has(node.id) ? "is-dimmed" : ""}`}
                    transform={`translate(${node.x} ${node.y})`}
                    role="button"
                    tabIndex={0}
                    aria-label={`${node.tracked ? "Saved address" : "Counterparty"}: ${node.address} on ${node.network}`}
                    aria-pressed={selectedNode === node.id}
                    onClick={() =>
                      setSelectedNode((current) =>
                        current === node.id ? "" : node.id,
                      )
                    }
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        setSelectedNode((current) =>
                          current === node.id ? "" : node.id,
                        );
                      }
                    }}
                  >
                    <circle className="flow-node-halo" r="30" />
                    <circle className="flow-node-core" r="9" />
                    <text
                      className="flow-node-label"
                      y="31"
                      textAnchor="middle"
                    >
                      {node.label}
                    </text>
                  </g>
                ))}
              </g>
            </svg>
          </div>
          {focusedNode && (
            <div className="flow-node-details" aria-live="polite">
              <div>
                <span>
                  {focusedNode.tracked ? "Saved address" : "Counterparty"} ·{" "}
                  {focusedNode.network}
                </span>
                <code>{focusedNode.address}</code>
              </div>
              <ul>
                {focusedEdges.slice(0, 5).map((edge) => {
                  const otherId =
                    edge.from === focusedNode.id ? edge.to : edge.from;
                  const other = nodesById.get(otherId);
                  return (
                    <li key={edge.id}>
                      <span>
                        {edge.from === focusedNode.id
                          ? "Sent to"
                          : "Received from"}{" "}
                        {other ? shortAddress(other.address) : "unknown"}
                      </span>
                      <strong>
                        {edge.amount} {edge.asset}{" "}
                        <small>· {edge.transfers} events</small>
                      </strong>
                      <small className="flow-edge-provenance">
                        {edge.providers.join(", ")} · latest{" "}
                        {edge.latestTimeKind === "block"
                          ? "block time"
                          : "provider observation"}{" "}
                        <time dateTime={edge.latestAt}>
                          {formatObservedAt(edge.latestAt)}
                        </time>
                        {edge.sourceHosts.length > 0 && (
                          <> · {edge.sourceHosts.join(", ")}</>
                        )}
                      </small>
                    </li>
                  );
                })}
              </ul>
            </div>
          )}
          {focusedNode && pathAnalysis && (
            <div className="flow-path-results" aria-live="polite">
              <div className="flow-path-results-heading">
                <strong>Shortest observed paths</strong>
                <span>
                  up to {maxDepth} hops · direction follows sender → recipient
                </span>
              </div>
              {pathAnalysis.paths.length === 0 ? (
                <p>No outgoing paths were observed within this depth.</p>
              ) : (
                <ol>
                  {pathAnalysis.paths.slice(0, 12).map((path) => {
                    const target = nodesById.get(path.targetId);
                    const steps = path.edgeIds
                      .map((edgeId) => edgesById.get(edgeId))
                      .filter((edge) => edge !== undefined);
                    if (!target) return null;
                    return (
                      <li key={path.targetId}>
                        <div className="flow-observed-route">
                          <span>{focusedNode.label}</span>
                          {steps.map((edge) => (
                            <span key={edge.id}>
                              <i aria-hidden="true">→</i>{" "}
                              {nodesById.get(edge.to)?.label ??
                                "Unknown address"}
                            </span>
                          ))}
                        </div>
                        <div className="flow-observed-evidence">
                          {steps.map((edge) => (
                            <span key={edge.id}>
                              {edge.asset} · {edge.transfers} observed event
                              {edge.transfers === 1 ? "" : "s"} ·{" "}
                              {edge.providers.join(", ")} · latest{" "}
                              {edge.latestTimeKind === "block"
                                ? "block time"
                                : "provider observation"}{" "}
                              <time dateTime={edge.latestAt}>
                                {formatObservedAt(edge.latestAt)}
                              </time>
                              {edge.sourceHosts.length > 0 &&
                                ` · ${edge.sourceHosts.join(", ")}`}
                            </span>
                          ))}
                        </div>
                      </li>
                    );
                  })}
                </ol>
              )}
              {pathAnalysis.paths.length > 12 && (
                <p>
                  Showing 12 of {pathAnalysis.paths.length} reachable addresses;
                  the graph highlights one selected shortest path per reachable
                  address within the selected depth.
                </p>
              )}
            </div>
          )}
          {graph.truncated && (
            <p className="flow-graph-truncated" role="status">
              The graph reached its display limit. It shows a bounded subset of
              the saved history.
            </p>
          )}
        </>
      )}
    </section>
  );
}

function formatObservedAt(value: string): string {
  const timestamp = new Date(value);
  return Number.isFinite(timestamp.getTime())
    ? new Intl.DateTimeFormat(undefined, {
        dateStyle: "medium",
        timeStyle: "short",
      }).format(timestamp)
    : "time unavailable";
}
