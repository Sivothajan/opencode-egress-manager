import { describe, expect, it } from "vitest";
import {
  cleanEndpointSchema,
  egressModeSchema,
  edgeOverrideSchema,
  parseProxyPool,
  proxyPoolSchema,
  validate,
} from "./validation";

describe("parseProxyPool", () => {
  it("parses comma/newline/semicolon separated nodes", () => {
    const parsed = parseProxyPool(
      "socks5://u:p@1.2.3.4:1080, http://5.6.7.8:8080\n9.9.9.9:7890;10.0.0.1:1080",
    );
    expect(parsed).toHaveLength(4);
    expect(parsed[0]).toMatchObject({
      host: "1.2.3.4",
      port: 1080,
      type: "socks5",
      hasCredentials: true,
    });
    expect(parsed[1].type).toBe("http");
    expect(parsed[2]).toMatchObject({ host: "9.9.9.9", port: 7890, hasCredentials: false });
  });

  it("ignores invalid tokens instead of throwing", () => {
    expect(parseProxyPool("not a proxy")).toEqual([]);
    expect(parseProxyPool("1.2.3.4:0")).toEqual([]);
    expect(parseProxyPool("1.2.3.4:70000")).toEqual([]);
    expect(parseProxyPool("[2001:db8::1]:1080")[0]).toMatchObject({
      host: "2001:db8::1",
      port: 1080,
    });
  });
});

describe("schemas", () => {
  it("accepts only known egress modes", () => {
    expect(validate(egressModeSchema, "warp").success).toBe(true);
    expect(validate(egressModeSchema, "vpn").success).toBe(false);
  });

  it("rejects malformed endpoints", () => {
    expect(validate(cleanEndpointSchema, "162.159.192.1:2408").success).toBe(true);
    expect(validate(cleanEndpointSchema, "162.159.192.1").success).toBe(false);
    expect(validate(cleanEndpointSchema, "999.1.1.1:2408").success).toBe(false);
    expect(validate(cleanEndpointSchema, "1.2.3.4:0").success).toBe(false);
    expect(validate(cleanEndpointSchema, "1.2.3.4:2408; rm -rf").success).toBe(false);
  });

  it("accepts auto or IPv6 for edge overrides", () => {
    expect(validate(edgeOverrideSchema, "auto").success).toBe(true);
    expect(validate(edgeOverrideSchema, "2606:4700::6810:7f60").success).toBe(true);
    expect(validate(edgeOverrideSchema, "../../etc").success).toBe(false);
    expect(validate(edgeOverrideSchema, "1.2.3.4").success).toBe(false);
  });

  it("bounds proxy pool length", () => {
    expect(proxyPoolSchema.safeParse("").success).toBe(false);
    expect(proxyPoolSchema.safeParse("a".repeat(5000)).success).toBe(false);
    expect(proxyPoolSchema.safeParse("127.0.0.1:1080").success).toBe(true);
  });

  it("rejects command-injection and NUL payloads", () => {
    expect(cleanEndpointSchema.safeParse("1.2.3.4:2408 && calc.exe").success).toBe(false);
    expect(cleanEndpointSchema.safeParse("1.2.3.4:2408\nnet user").success).toBe(false);
    expect(edgeOverrideSchema.safeParse("auto; rm -rf /").success).toBe(false);
    expect(proxyPoolSchema.safeParse("127.0.0.1:1080\u0000").success).toBe(false);
  });
});
