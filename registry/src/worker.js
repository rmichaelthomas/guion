import { handle } from "./core.js";

export default {
  fetch(request, env) {
    return handle(request, env);
  },
};
