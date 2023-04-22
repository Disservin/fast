<template>
  <div>
    <v-btn class="btn" color="primary" v-bind="$attrs">
      <font-awesome-icon icon="fa-solid fa-clipboard" />
      <span>Copy PGN</span>
    </v-btn>
  </div>
</template>

<script>
import { defineComponent } from "vue";

export default defineComponent({
  name: "AppBtn",
  props: {},
  inheritAttrs: false,
});
</script>

<style scoped>
.btn {
  border-radius: 5px;
  padding: 0.5rem;
  margin: 0.5rem;
  width: 100%;
}
.btn-wrap {
  display: flex;
  justify-content: center;
  background-color: var(--bg-tertiary);
  margin: 5px;
}
</style>
