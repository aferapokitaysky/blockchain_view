import { useMemo, useState } from "react";
import type { WalletHistory } from "../investigations/api";
import { buildWalletFlowGraph } from "./model";

type WalletFlowGraphProps = {
  histories: WalletHistory[];
};

function shortAddress(address: string): string {
  return `${address.slice(0, 8)}…${address.slice(-6)}`;
}

export function WalletFlowGraph({ histories }: WalletFlowGraphProps) {
  const graph = useMemo(() => buildWalletFlowGraph(histories), [histories]);
  const [selectedNode, setSelectedNode] = useState("");
  const focusedNode = graph.nodes.find((node) => node.id === selectedNode);
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
                  const from = graph.nodes.find(
                    (node) => node.id === edge.from,
                  );
                  const to = graph.nodes.find((node) => node.id === edge.to);
                  if (!from || !to) return null;
                  const offset = ((index % 5) - 2) * 16;
                  const midX = (from.x + to.x) / 2;
                  const midY = (from.y + to.y) / 2 + offset;
                  return (
                    <g key={edge.id}>
                      <title>
                        {`${shortAddress(from.address)} to ${shortAddress(to.address)}: ${edge.amount} ${edge.asset}, ${edge.transfers} event${edge.transfers === 1 ? "" : "s"}`}
                      </title>
                      <path
                        className="flow-edge-line"
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
                    className={`flow-node ${node.tracked ? "is-tracked" : "is-external"} ${selectedNode === node.id ? "is-selected" : ""}`}
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
                  const other = graph.nodes.find((node) => node.id === otherId);
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
                    </li>
                  );
                })}
              </ul>
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
