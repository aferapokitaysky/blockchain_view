import type { GraphSnapshot } from "../investigations/api";
import { isEvmNetworkId } from "../networks/networkCatalog";

export type SnapshotViewState = {
  selectedNodeId: string;
  maxDepth: 1 | 2 | 3;
};

export function getSnapshotViewState(
  snapshot: GraphSnapshot | null,
): SnapshotViewState {
  if (!snapshot) return { selectedNodeId: "", maxDepth: 2 };

  const focusedNode =
    snapshot.focus_network && snapshot.focus_address
      ? snapshot.graph.nodes.find(
          (node) =>
            node.network === snapshot.focus_network &&
            (isEvmNetworkId(node.network)
              ? node.address.toLowerCase() ===
                snapshot.focus_address?.toLowerCase()
              : node.address === snapshot.focus_address),
        )
      : undefined;

  return {
    selectedNodeId: focusedNode?.id ?? "",
    maxDepth: snapshot.max_depth,
  };
}
