import type { Addresses } from "./api";

/** What CSCM's GET /api/servers returns about how to reach a server. */
export interface CscmNetworking {
  type: string;
  port: number;
  tunnels?: { address: string | null; external_port: number | null }[];
  dns_records?: { type: string; name: string }[];
}

/**
 * How players reach a CSCM server, same rules as the CSCM dashboard and the launcher's Rust
 * side (servers::addresses): the custom domain once its SRV record carries the port, else
 * the Playit address; and host:port for players on the machine's network.
 */
export function serverAddresses(s: CscmNetworking, machineUrl: string): Addresses {
  const bedrock = s.type === "bedrock";
  const cname = s.dns_records?.find((r) => r.type === "CNAME")?.name;
  const srvReady = bedrock || !!s.dns_records?.some((r) => r.type === "SRV");
  const t = s.tunnels?.find((t) => t.address);
  // Bedrock always needs the port; Java's raw tunnel host (*.ply.gg) too, only joinmc.link names carry it in SRV
  const needsPort = bedrock || !t?.address?.endsWith("joinmc.link");
  const playit = t?.address ? (needsPort && t.external_port && !t.address.includes(":") ? `${t.address}:${t.external_port}` : t.address) : null;
  const custom = cname && srvReady ? cname : null;
  let host: string | null = null;
  try {
    host = new URL(machineUrl).hostname;
  } catch {
    // keep null
  }
  return {
    public: custom ?? playit,
    playit: custom ? playit : null,
    lan: host ? (s.port === 25565 ? host : `${host}:${s.port}`) : null,
  };
}
