use super::*;

pub(super) fn http_demo_functions_with_runtime_delay(
    runtime_schedule_delay_ms: u64,
) -> serde_json::Value {
    let send_and_schedule_handler = format!(
        "async (ctx, {{ author, body }}) => {{\n    const id = await ctx.db.insert(\"messages\", {{ author, body }});\n    await ctx.scheduler.runAfter(\n      {runtime_schedule_delay_ms},\n      internalScheduledFunctions.messages.sendInternal,\n      {{ author, body: `${{body}} (scheduled)` }},\n    );\n    return id;\n  }}"
    );
    json!([
        {
            "name": "messages:byAuthor",
            "export": "byAuthor",
            "module": "messages",
            "kind": "query",
            "visibility": "public",
            "schedulable": false,
            "plan": {
                "table": "messages",
                "filters": [
                    {
                        "field": "author",
                        "op": "eq",
                        "value": { "$arg": "author" }
                    }
                ],
                "order": null,
                "limit": 20
            },
            "runtime_handler": null
        },
        {
            "name": "messages:maybeByAuthor",
            "export": "maybeByAuthor",
            "module": "messages",
            "kind": "query",
            "visibility": "public",
            "schedulable": false,
            "plan": null,
            "runtime_handler": "async (ctx, { author }) => {\n    const messages = author\n      ? await ctx.db\n        .query(\"messages\")\n        .withIndex(\"by_author\", (q) => q.eq(\"author\", author))\n        .take(20)\n      : await ctx.db.query(\"messages\").take(20);\n    return messages.slice(0, 20);\n  }",
            "runtime_bindings": {
                "internalScheduledFunctions": {
                    "type": "generated_reference_tree",
                    "visibility": "internal",
                    "reference_kind": "mutation"
                },
                "internal": {
                    "type": "generated_reference_tree",
                    "visibility": "internal"
                }
            }
        },
        {
            "name": "messages:byId",
            "export": "byId",
            "module": "messages",
            "kind": "query",
            "visibility": "public",
            "schedulable": false,
            "plan": {
                "type": "get",
                "table": "messages",
                "id": { "$arg": "id" }
            },
            "runtime_handler": null
        },
        {
            "name": "messages:uniqueByAuthor",
            "export": "uniqueByAuthor",
            "module": "messages",
            "kind": "query",
            "visibility": "public",
            "schedulable": false,
            "plan": {
                "type": "unique",
                "query": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": null,
                    "limit": 2
                }
            },
            "runtime_handler": null
        },
        {
            "name": "messages:exactByAuthorAndBody",
            "export": "exactByAuthorAndBody",
            "module": "messages",
            "kind": "query",
            "visibility": "public",
            "schedulable": false,
            "plan": {
                "type": "unique",
                "query": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        },
                        {
                            "field": "body",
                            "op": "eq",
                            "value": { "$arg": "body" }
                        }
                    ],
                    "order": null,
                    "limit": 2
                }
            },
            "runtime_handler": null
        },
        {
            "name": "messages:sendInternal",
            "export": "sendInternal",
            "module": "messages",
            "kind": "mutation",
            "visibility": "internal",
            "schedulable": true,
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            },
            "runtime_handler": null
        },
        {
            "name": "messages:sendViaAction",
            "export": "sendViaAction",
            "module": "messages",
            "kind": "action",
            "visibility": "public",
            "schedulable": false,
            "plan": {
                "type": "call_mutation",
                "name": "messages:sendInternal",
                "visibility": "internal",
                "args": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            },
            "runtime_handler": null
        },
        {
            "name": "messages:scheduleSend",
            "export": "scheduleSend",
            "module": "messages",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": {
                "type": "schedule_run_after",
                "delay_ms": { "$arg": "delayMs" },
                "name": "messages:sendInternal",
                "visibility": "internal",
                "args": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            },
            "runtime_handler": null
        },
        {
            "name": "messages:sendAndSchedule",
            "export": "sendAndSchedule",
            "module": "messages",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": null,
            "runtime_handler": send_and_schedule_handler,
            "runtime_bindings": {
                "internalScheduledFunctions": {
                    "type": "generated_reference_tree",
                    "visibility": "internal",
                    "reference_kind": "mutation"
                },
                "internal": {
                    "type": "generated_reference_tree",
                    "visibility": "internal"
                }
            }
        }
    ])
}

pub(super) fn http_demo_routes() -> serde_json::Value {
    json!([
        {
            "method": "POST",
            "plan": {
                "type": "http_response",
                "response": {
                    "kind": "json",
                    "body": {
                        "id": {
                            "$result": {
                                "index": 0,
                                "path": ""
                            }
                        }
                    },
                    "status": 201
                },
                "operation": {
                    "type": "call_mutation",
                    "name": "messages:sendInternal",
                    "visibility": "internal",
                    "args": {
                        "author": {
                            "$request": {
                                "source": "json",
                                "path": "author"
                            }
                        },
                        "body": {
                            "$request": {
                                "source": "json",
                                "path": "body"
                            }
                        }
                    }
                }
            },
            "path": "/messages",
            "name": "http:inline:0"
        },
        {
            "method": "GET",
            "plan": {
                "type": "http_response",
                "response": {
                    "kind": "json",
                    "body": {
                        "$result": {
                            "index": 0,
                            "path": ""
                        }
                    }
                },
                "operation": {
                    "type": "call_query",
                    "name": "messages:byAuthor",
                    "visibility": "public",
                    "args": {
                        "author": {
                            "$request": {
                                "source": "query",
                                "name": "author"
                            }
                        }
                    }
                }
            },
            "path": "/messages/by-author",
            "name": "http:inline:1"
        }
    ])
}

pub(super) fn http_demo_schema() -> serde_json::Value {
    json!({
        "tables": {
            "messages": {
                "fields": {
                    "author": { "kind": "string" },
                    "body": { "kind": "string" }
                },
                "indexes": [
                    { "name": "by_author", "fields": ["author"] }
                ]
            }
        }
    })
}
