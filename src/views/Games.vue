<template lang="">
  <main>
    <div>
      <h1>Games</h1>
    </div>
  </main>
</template>
<script>
import { invoke } from "@tauri-apps/api";
import { defineComponent } from "vue";

export default defineComponent({
  name: "Games",
  mounted() {
    this.test();
  },
  methods: {
    test() {
      invoke("new", { command: "./stockfish.exe" }).then((res) => {
        invoke("go_nodes", { nodes: 100 }).then((res) => {
          invoke("get_bestmove").then((res) => {
            console.log(res);
          });
        });
      });
    },
  },
  destroyed() {
    invoke("quit").then((response) => console.log(response));
  },
});
</script>

<style scoped>
main {
  margin-left: 5rem;
}

h1 {
  color: #fff;
}
</style>
