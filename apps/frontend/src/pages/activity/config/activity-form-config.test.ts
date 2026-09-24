import { describe, expect, it } from "vitest";
import { ActivityType } from "@/lib/constants";
import { mapActivityTypeToPicker } from "../utils/activity-form-utils";
import { ACTIVITY_FORM_CONFIG, hasActivityForm } from "./activity-form-config";

describe("hasActivityForm", () => {
  it("accepts every type the picker can offer", () => {
    for (const pickerType of [
      ActivityType.BUY,
      ActivityType.SELL,
      ActivityType.DEPOSIT,
      ActivityType.WITHDRAWAL,
      ActivityType.DIVIDEND,
      "TRANSFER",
      ActivityType.SPLIT,
      ActivityType.FEE,
      ActivityType.INTEREST,
      ActivityType.TAX,
      ActivityType.CREDIT,
    ]) {
      expect(hasActivityForm(pickerType)).toBe(true);
    }
  });

  it("accepts ADJUSTMENT, which is editable without being offered for creation", () => {
    expect(hasActivityForm(ActivityType.ADJUSTMENT)).toBe(true);
  });

  it("rejects a stored type that has no editor", () => {
    // A needs-review row imported by sync arrives as UNKNOWN, which carries no
    // classification and so has nothing to edit — the caller must offer the
    // picker rather than pin it.
    expect(hasActivityForm(ActivityType.UNKNOWN)).toBe(false);
  });

  it("rejects an absent type", () => {
    expect(hasActivityForm(undefined)).toBe(false);
    expect(hasActivityForm("")).toBe(false);
  });

  it("agrees with the picker mapping for both transfer legs", () => {
    // TRANSFER_IN/OUT are stored types with no form of their own; the picker
    // alias is what has one, so the two helpers have to be used together.
    expect(hasActivityForm(ActivityType.TRANSFER_IN)).toBe(false);
    expect(hasActivityForm(mapActivityTypeToPicker(ActivityType.TRANSFER_IN))).toBe(true);
    expect(hasActivityForm(mapActivityTypeToPicker(ActivityType.TRANSFER_OUT))).toBe(true);
  });
});

describe("prefilled asset", () => {
  // Opening the form from an asset page (?assetId=…) must keep that exact asset:
  // with only the symbol, two assets sharing it made the save land on the other.
  it.each([ActivityType.BUY, ActivityType.SELL])("%s submits the prefilled asset id", (type) => {
    const config = ACTIVITY_FORM_CONFIG[type];
    const defaults = config.getDefaults(
      { assetId: "bond-orig", assetSymbol: "BYMA-CAC5O", instrumentType: "BOND" },
      [],
    );

    expect(defaults).toMatchObject({ existingAssetId: "bond-orig" });
    expect(config.toPayload(defaults as never)).toMatchObject({
      assetId: "BYMA-CAC5O",
      existingAssetId: "bond-orig",
      symbolInstrumentType: "BOND",
    });
  });
});
