import { createApp } from "vue";
import ArcoVue from "@arco-design/web-vue";
import ArcoVueIcon from "@arco-design/web-vue/es/icon";
import "@arco-design/web-vue/dist/arco.css";
import App from "./App.vue";
import i18n from "./i18n";
import "./styles/global.scss";

const app = createApp(App);
app.use(ArcoVue);
app.use(ArcoVueIcon);
app.use(i18n);
app.mount("#app");
