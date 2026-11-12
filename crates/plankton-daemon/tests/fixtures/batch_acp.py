"""Protocol fixture: submits a result file while emitting unrelated chat text."""
import json
import pathlib
import sys

log_path = pathlib.Path(sys.argv[1])
mode = sys.argv[2]
session = 0
surfaces = ["llm_context", "network", "local_persistence", "terminal_log", "process_propagation"]

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        result = {"protocolVersion": 1, "agentCapabilities": {}, "agentInfo": {"name": "fixture-acp", "version": "1"}}
    elif method == "session/new":
        session += 1
        result = {"sessionId": str(session)}
    elif method == "session/prompt":
        params = message["params"]
        if "direct" in mode:
            # Adapters may emit ordinary analysis before tools, without phase metadata.
            for update in [
                {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Inspecting evidence first. {not a decision}"}},
                {"sessionUpdate": "tool_call", "toolCallId": "read-1", "title": "Read evidence", "kind": "read", "status": "completed"},
            ]:
                print(json.dumps({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": params["sessionId"], "update": update}}), flush=True)
        prompt = "".join(block.get("text", "") for block in params["prompt"])
        payload = json.loads(prompt.split("Request evidence:\n", 1)[1].split("\n\nResult schema:", 1)[0])
        with log_path.open("a") as log:
            log.write(json.dumps([item["request_id"] for item in payload["requests"]]) + "\n")
        reviews = [{"request_id": item["request_id"], "decision": {
            "suggested_decision": "deny" if item["context"]["resource"].endswith("deny") else "allow",
            "rationale_summary": "independent fixture decision", "risk_score": 0,
            "exposure_report": {"chain_summary": "synthetic empty chain", "surfaces": [
                {"surface": surface, "actual_level": 0, "evidence_state": "not_observed", "summary": "synthetic evidence"}
                for surface in surfaces]},
        }} for item in payload["requests"]]
        if mode == "missing":
            reviews.pop()
        text = json.dumps({"reviews": reviews})
        if "direct" not in mode:
            output_path = log_path.parent / ("result-" + params["sessionId"] + ".json")
            output_path.write_text(text)
            text = json.dumps({"result_file": str(output_path)})
        print(json.dumps({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": params["sessionId"], "update": {"sessionUpdate": "agent_message_chunk",
                "content": {"type": "text", "text": text}}}}), flush=True)
        result = {"stopReason": "end_turn"}
    else:
        result = {}
    if "id" in message:
        print(json.dumps({"jsonrpc": "2.0", "id": message["id"], "result": result}), flush=True)
