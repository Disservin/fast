<template>
  <main>
    <h1>Engines</h1>
  </main>
</template>

<script lang="ts">
import { invoke } from "@tauri-apps/api";

import { defineComponent } from "vue";

export default defineComponent({
  name: "Engines",
  data() {
    return {};
  },
  mounted() {
    this.test();
    console.log("ready");
  },
  methods: {
    test() {
      //   invoke("greet")
      //     .then((response) => console.log(response));
      invoke("start_process", {
        command: "./stockfish.exe",
        args: ["bench"],
      }).then((response) =>
        invoke("send_string", { input: "bench" }).then((response) =>
          console.log(response)
        )
      );
    },
  },
  destroyed() {
    invoke("end_process").then((response) => console.log(response));
  },
});
</script>
