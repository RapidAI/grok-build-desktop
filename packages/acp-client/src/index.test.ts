import test from "node:test";
import assert from "node:assert/strict";
import { encodeAcpLine, feedAcpLines, isRequest, isResponse } from "./index.ts";

test("encodeAcpLine ends with newline", () => {
  const line = encodeAcpLine({
    jsonrpc: "2.0",
    id: 1,
    method: "initialize",
    params: {},
  });
  assert.equal(line.endsWith("\n"), true);
  assert.equal(line.includes("\n\n"), false);
});

test("feedAcpLines splits complete frames and keeps rest", () => {
  const first = feedAcpLines("", '{"jsonrpc":"2.0","id":1,"result":{}}\n{"jsonrpc":"2.0",');
  assert.equal(first.messages.length, 1);
  assert.equal(isResponse(first.messages[0]!), true);
  assert.equal(first.rest.startsWith('{"jsonrpc"'), true);

  const second = feedAcpLines(
    first.rest,
    '"id":2,"method":"session/update","params":{}}\n',
  );
  assert.equal(second.messages.length, 1);
  assert.equal(isRequest(second.messages[0]!) || "method" in second.messages[0]!, true);
  assert.equal(second.rest, "");
});
