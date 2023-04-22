<template>
  <div class="btn-wrap">
    <v-btn class="btn" color="primary" v-bind="$attrs">
      <font-awesome-icon icon="fa-solid fa-clipboard" />
      <span>{{ text }}</span>
    </v-btn>
  </div>
</template>

<script>
import { defineComponent } from "vue";

export default defineComponent({
  name: "AppBtn",
  props: {
    text: {
      type: String,
      required: true,
    },
  },
  inheritAttrs: false,
});
</script>

<style scoped>
.btn {
  border-radius: 5px;
  padding: 0.5rem;
  margin: 0.5rem;
  flex: auto;
}
.btn-wrap {
  display: flex;
  flex-direction: row;
  justify-content: space-between;
  background-color: var(--bg-tertiary);
  margin: 5px;
}
</style>
