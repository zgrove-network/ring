import { onRequest as __api_blocks_ts_onRequest } from "/Users/ercanilbars/Desktop/zeroptis/zgrove-ring/app/functions/api/blocks.ts"

export const routes = [
    {
      routePath: "/api/blocks",
      mountPath: "/api",
      method: "",
      middlewares: [],
      modules: [__api_blocks_ts_onRequest],
    },
  ]