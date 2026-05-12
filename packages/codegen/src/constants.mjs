const ARG_MARKER = "__nimbusConvexArg";
const OPERATION_MARKER = "__nimbusConvexOperation";
const QUERY_STATE_MARKER = "__nimbusConvexQueryState";
const REQUEST_MARKER = "__nimbusConvexRequest";
const RESULT_MARKER = "__nimbusConvexResult";
const HTTP_RESPONSE_MARKER = "__nimbusConvexHttpResponse";

const DEFINE_HELPERS = new Map([
  ["defineQuery", { kind: "query", visibility: "public", mode: "define" }],
  [
    "definePaginatedQuery",
    { kind: "paginated_query", visibility: "public", mode: "define" },
  ],
  ["defineMutation", { kind: "mutation", visibility: "public", mode: "define" }],
  ["defineAction", { kind: "action", visibility: "public", mode: "define" }],
]);

const SERVER_HELPERS = new Map([
  ["query", { kind: "query", visibility: "public", mode: "server" }],
  [
    "paginatedQuery",
    { kind: "paginated_query", visibility: "public", mode: "server" },
  ],
  ["mutation", { kind: "mutation", visibility: "public", mode: "server" }],
  ["action", { kind: "action", visibility: "public", mode: "server" }],
  ["internalQuery", { kind: "query", visibility: "internal", mode: "server" }],
  [
    "internalPaginatedQuery",
    { kind: "paginated_query", visibility: "internal", mode: "server" },
  ],
  [
    "internalMutation",
    { kind: "mutation", visibility: "internal", mode: "server" },
  ],
  ["internalAction", { kind: "action", visibility: "internal", mode: "server" }],
  ["httpAction", { kind: "http_action", visibility: "public", mode: "server" }],
]);

const SUPPORTED_HELPERS = new Map([...DEFINE_HELPERS, ...SERVER_HELPERS]);

export {
  ARG_MARKER,
  DEFINE_HELPERS,
  HTTP_RESPONSE_MARKER,
  OPERATION_MARKER,
  QUERY_STATE_MARKER,
  REQUEST_MARKER,
  RESULT_MARKER,
  SERVER_HELPERS,
  SUPPORTED_HELPERS,
};
