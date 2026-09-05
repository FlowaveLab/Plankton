import { describe, expect, it } from "vitest";

import {
  groupPasswordChangeItems,
  type PasswordChangeItemDiff,
} from "./passwordChangePresentation";

function titleDiff(
  recordId: string,
  itemId: string,
  after: string,
): PasswordChangeItemDiff {
  return {
    record_id: recordId,
    item_id: itemId,
    title: after,
    entries: [
      {
        path: "/title",
        label: "Title",
        before: ".env",
        after,
        impact: "metadata",
      },
    ],
  };
}

describe("groupPasswordChangeItems", () => {
  it("groups field-level records by the same logical password item", () => {
    const grouped = groupPasswordChangeItems([
      titleDiff("record-1", "item-example", "示例内部服务凭据"),
      titleDiff("record-2", "item-example", "示例内部服务凭据"),
      titleDiff("record-3", "item-example", "示例内部服务凭据"),
    ]);

    expect(grouped).toEqual([
      {
        item_id: "item-example",
        title: "示例内部服务凭据",
        record_ids: ["record-1", "record-2", "record-3"],
        vaults: [],
        entries: [
          {
            path: "/title",
            label: "Title",
            before: ".env",
            after: "示例内部服务凭据",
            impact: "metadata",
          },
        ],
      },
    ]);
  });

  it("keeps distinct changes inside one logical password item", () => {
    const grouped = groupPasswordChangeItems([
      titleDiff("record-1", "item-1", "Deploy credential"),
      {
        record_id: "record-2",
        item_id: "item-1",
        title: "Deploy credential",
        entries: [
          {
            path: "/fields/token/resource_id",
            label: "Token resource",
            before: "secret/old-token",
            after: "secret/deploy-token",
            impact: "references",
          },
        ],
      },
    ]);

    expect(grouped).toHaveLength(1);
    expect(grouped[0]?.entries).toHaveLength(2);
    expect(grouped[0]?.record_ids).toEqual(["record-1", "record-2"]);
  });

  it("does not merge different logical items that share a title", () => {
    const grouped = groupPasswordChangeItems([
      titleDiff("record-1", "item-1", "Credential"),
      titleDiff("record-2", "item-2", "Credential"),
    ]);

    expect(grouped.map((item) => item.item_id)).toEqual(["item-1", "item-2"]);
  });
});
