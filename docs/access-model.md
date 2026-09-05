# Access and approval model

[← README](../README.md)

## From a secret request to an approved action

1. ** An agent needs a credential.** The `secret-access` Skill uses metadata-only discovery to find a resource, then requests access with a reason.
2. ** Plankton captures the call chain.** Process ancestry, command arguments, script references, and request context give the reviewer evidence about the intended use. OS observations and requester claims retain their provenance.
3. ** A human or LLM reviews the request.** Human Review puts the decision in the desktop app. Assisted mode adds LLM advice. Auto mode evaluates that advice against locally enforced exposure policy.
4. ** Plankton applies the outcome.** An allowed request resolves the value for the consumer. A denial releases no value. Escalation leaves the request for a human decision.
5. ** The decision stays inspectable.** The desktop retains request history, reasons, and evidence. Detailed AI audit annotations follow the decision without delaying an already approved read.

| Mode             | Who reviews?                                                     | Who makes the final decision?                                                       |
| ---------------- | ---------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| **Human Review** | A human inspects the request in the desktop UI.                  | The human approves or rejects; no model provider is needed.                         |
| **Assisted**     | An LLM investigates and offers a recommendation.                 | The human decides in the desktop UI.                                                |
| **Auto**         | An LLM assesses the evidence against the saved exposure profile. | Plankton validates the response and applies local policy: allow, deny, or escalate. |

The requesting agent and the reviewing LLM are **separate roles**. A pending request can still be decided by a human while AI review is running; later model output cannot overwrite that decision. These diagrams describe **Protected** credentials. A field explicitly configured as **Direct** skips human and LLM approval while retaining an access-request record.

### You define the exposure scope

A credential's exposure profile answers five concrete questions. Collections have a default profile; fields inherit it or use a human-configured override. Agents may suggest a profile in an import draft, and the human can edit it before confirming the save.

| Exposure surface        | What the human controls                                               | Example policy for an API token   |
| ----------------------- | --------------------------------------------------------------------- | --------------------------------- |
| **LLM context**         | Whether the credential may enter model-visible context.               | Do not put it in prompts or chat. |
| **Network**             | Whether it may leave the machine, and which destinations are allowed. | Only `api.example.com`.           |
| **Local persistence**   | Whether the credential may be written to local storage.               | No files or caches.               |
| **Terminal & logs**     | Whether the credential may appear in output or logs.                  | No echoing or logging.            |
| **Process propagation** | Whether it may be handed to another local process.                    | Only the declared consumer.       |

This is an **illustrative profile**, not the default: protected credentials start with network exposure disabled. The LLM reports observed exposure and its evidence; local code checks levels, uncertainty, and destination rules. Evidence that is unknown cannot automatically allow access. Out-of-scope use is denied or sent to human review according to policy; a low model risk score is not an approval threshold.

## Trust & data boundaries

Plankton is an approval layer for a **trusted local computer**. Call-chain evidence supports review; it is not a sandbox or proof of everything a process will do after receiving a credential.

- **Approved output is sensitive.** A successful `plankton get` emits the raw value. The Skill directs agents to pipe it to a consumer that does not echo or log it; standalone `get` in a model-visible terminal would disclose it.
- **Evidence can contain sensitive context.** Current reviewer input and audit records preserve arguments, supplied environment values, metadata, and source evidence without redaction. Keep credentials out of these fields. The normal review payload does not resolve the requested vault value for the model.
- **LLM reviewers are trusted tools.** Review agents can inspect files and run commands with broad access. Choose the provider and execution environment accordingly; local-first does not mean every review runs offline.
- **Local checks govern automatic release.** Invalid model output, missing decision evidence, and policy violations cannot silently become automatic approval. Humans control policy and final desktop confirmation for password changes.
- **Vault sync transfers encrypted data.** It synchronizes KDBX bytes and non-secret revision/hash metadata, excluding local unlock files. It does not provide remote-server approval or execution.

Read the [automatic approval contract](./automatic-approval.md) for exact evidence, tool-access, outcome-routing, and audit semantics.
