<script lang="ts">
export default {
  props: {
    status: {
      type: Boolean,
      required: true,
    },
    go: {
      type: Boolean,
      required: true,
    },
  },
  data() {
    return {
      // stopped, running
      isRunning: this.status,
      disabled: false,
      disabledGo: false,
    };
  },
  mounted() {
    this.isRunning = localStorage.getItem("status") === "true";
    this.disabledGo = !this.go;
  },
  methods: {
    sendEngineCommand(command: string) {
      if (command === "go") {
        this.isRunning = true;
      } else if (command === "stop") {
        this.isRunning = false;
      } else if (command === "restart") {
        this.disabled = true;
        setTimeout(() => {
          this.disabled = false;
        }, 5000);
      }
      this.$emit("engine-command", command);

      localStorage.setItem("status", this.isRunning.toString());
    },
  },
};
</script>

<template lang="">
  <div class="buttons">
    <v-row justify="center" class="mt-4 mb-4 mx-2">
      <v-col cols="4">
        <v-btn
          class="mr-2"
          outlined
          color="primary"
          block
          :disabled="isRunning || disabledGo"
          @click="sendEngineCommand('go')"
        >
          Go Infinite
        </v-btn>
      </v-col>
      <v-col cols="4">
        <v-btn
          class="mr-2"
          outlined
          color="primary"
          block
          :disabled="!isRunning"
          @click="sendEngineCommand('stop')"
        >
          Stop
        </v-btn>
      </v-col>
      <v-col cols="4">
        <v-btn
          id="restart-button"
          outlined
          color="primary"
          block
          :disabled="isRunning || disabled"
          @click="sendEngineCommand('restart')"
        >
          Restart
        </v-btn>
      </v-col>
    </v-row>
  </div>
</template>

<style scoped>
.buttons {
  background-color: var(--bg-tertiary);
  margin: 5px;
}
</style>
