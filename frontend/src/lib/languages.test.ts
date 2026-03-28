import { describe, it, expect } from "vitest";
import { getLang } from "./languages";

describe("getLang", () => {
  describe("common web extensions", () => {
    it("maps .js to javascript", () => expect(getLang("app.js")).toBe("javascript"));
    it("maps .ts to typescript", () => expect(getLang("index.ts")).toBe("typescript"));
    it("maps .tsx to tsx", () => expect(getLang("App.tsx")).toBe("tsx"));
    it("maps .jsx to jsx", () => expect(getLang("App.jsx")).toBe("jsx"));
    it("maps .html to html", () => expect(getLang("index.html")).toBe("html"));
    it("maps .css to css", () => expect(getLang("style.css")).toBe("css"));
    it("maps .vue to vue", () => expect(getLang("App.vue")).toBe("vue"));
    it("maps .svelte to svelte", () => expect(getLang("App.svelte")).toBe("svelte"));
  });

  describe("systems languages", () => {
    it("maps .rs to rust", () => expect(getLang("main.rs")).toBe("rust"));
    it("maps .go to go", () => expect(getLang("main.go")).toBe("go"));
    it("maps .c to c", () => expect(getLang("main.c")).toBe("c"));
    it("maps .cpp to cpp", () => expect(getLang("main.cpp")).toBe("cpp"));
    it("maps .java to java", () => expect(getLang("Main.java")).toBe("java"));
    it("maps .swift to swift", () => expect(getLang("main.swift")).toBe("swift"));
    it("maps .py to python", () => expect(getLang("script.py")).toBe("python"));
  });

  describe("config and data formats", () => {
    it("maps .json to json", () => expect(getLang("data.json")).toBe("json"));
    it("maps .yaml to yaml", () => expect(getLang("config.yaml")).toBe("yaml"));
    it("maps .yml to yaml", () => expect(getLang("config.yml")).toBe("yaml"));
    it("maps .toml to toml", () => expect(getLang("Cargo.toml")).toBe("toml"));
    it("maps .xml to xml", () => expect(getLang("pom.xml")).toBe("xml"));
    it("maps .sql to sql", () => expect(getLang("schema.sql")).toBe("sql"));
  });

  describe("shell scripts", () => {
    it("maps .sh to shellscript", () => expect(getLang("deploy.sh")).toBe("shellscript"));
    it("maps .bash to shellscript", () => expect(getLang("run.bash")).toBe("shellscript"));
    it("maps .zsh to shellscript", () => expect(getLang("init.zsh")).toBe("shellscript"));
  });

  describe("extensionless convention files", () => {
    it("maps dockerfile to dockerfile", () => expect(getLang("dockerfile")).toBe("dockerfile"));
    it("maps makefile to makefile", () => expect(getLang("makefile")).toBe("makefile"));
    it("maps Dockerfile (capitalized) to dockerfile", () => expect(getLang("Dockerfile")).toBe("dockerfile"));
    it("maps Makefile (capitalized) to makefile", () => expect(getLang("Makefile")).toBe("makefile"));
  });

  describe("unknown files", () => {
    it("returns null for unknown extensions", () => {
      expect(getLang("data.bin")).toBeNull();
      expect(getLang("video.mp4")).toBeNull();
      expect(getLang("archive.zip")).toBeNull();
    });

    it("returns null for files with no extension and no convention match", () => {
      expect(getLang("LICENSE")).toBeNull();
      expect(getLang("CHANGELOG")).toBeNull();
    });
  });

  describe("case insensitivity", () => {
    it("handles uppercase extensions", () => {
      expect(getLang("readme.MD")).toBe("markdown");
      expect(getLang("style.CSS")).toBe("css");
    });
  });
});
