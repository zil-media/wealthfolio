import { describe, expect, it } from "vitest";
import { buildOccSymbol, normalizeOptionSymbol, parseOccSymbol } from "./occ-symbol";

describe("OCC expiration dates", () => {
  it.each(["2000-01-01", "2000-02-29", "2011-12-30", "2024-02-29", "2099-12-31"])(
    "preserves %s when building and parsing a contract",
    (expiration) => {
      const symbol = buildOccSymbol("AAPL", expiration, "CALL", 150);
      expect(parseOccSymbol(symbol)?.expiration).toBe(expiration);
    },
  );

  it.each(["0002-12-31", "1999-12-31", "2100-01-01", "2027-02-29", "2028-02-30", "2027-1-01"])(
    "rejects %s instead of building a different contract",
    (expiration) => {
      expect(() => buildOccSymbol("AAPL", expiration, "CALL", 150)).toThrow();
    },
  );

  it.each(["270229", "280230", "270001", "271301", "270100"])(
    "rejects impossible date %s in standard and broker symbols",
    (expiration) => {
      expect(parseOccSymbol(`AAPL${expiration}C00150000`)).toBeNull();
      expect(normalizeOptionSymbol(`-AAPL${expiration}C150`)).toBeNull();
    },
  );

  it("uses the same calendar validation for broker symbols", () => {
    const symbol = normalizeOptionSymbol("-AAPL240229C150");
    expect(symbol).toBe("AAPL240229C00150000");
    expect(parseOccSymbol(symbol!)?.expiration).toBe("2024-02-29");
  });
});
