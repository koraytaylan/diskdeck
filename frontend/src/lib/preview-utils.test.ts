import { describe, it, expect } from "vitest";
import { classifyForPreview, isArchive, MAX_PREVIEW_SIZE } from "./preview-utils";

describe("classifyForPreview", () => {
  describe("MIME type classification", () => {
    it("classifies image/* as image", () => {
      expect(classifyForPreview("image/png", "photo.png")).toBe("image");
      expect(classifyForPreview("image/jpeg", "photo.jpg")).toBe("image");
      expect(classifyForPreview("image/gif", "anim.gif")).toBe("image");
      expect(classifyForPreview("image/webp", "pic.webp")).toBe("image");
      expect(classifyForPreview("image/svg+xml", "icon.svg")).toBe("image");
    });

    it("classifies text/* as text", () => {
      expect(classifyForPreview("text/plain", "readme.txt")).toBe("text");
      expect(classifyForPreview("text/html", "index.html")).toBe("text");
      expect(classifyForPreview("text/css", "style.css")).toBe("text");
    });

    it("classifies application/json as text", () => {
      expect(classifyForPreview("application/json", "data.json")).toBe("text");
    });

    it("classifies application/xml as text", () => {
      expect(classifyForPreview("application/xml", "config.xml")).toBe("text");
    });

    it("classifies application/javascript as text", () => {
      expect(classifyForPreview("application/javascript", "app.js")).toBe("text");
    });

    it("classifies application/octet-stream as binary", () => {
      expect(classifyForPreview("application/octet-stream", "data.bin")).toBe("binary");
    });

    it("classifies application/pdf as binary", () => {
      expect(classifyForPreview("application/pdf", "doc.pdf")).toBe("binary");
    });
  });

  describe("extension fallback when MIME is null", () => {
    it("classifies code file extensions as text", () => {
      expect(classifyForPreview(null, "main.rs")).toBe("text");
      expect(classifyForPreview(null, "app.ts")).toBe("text");
      expect(classifyForPreview(null, "index.tsx")).toBe("text");
      expect(classifyForPreview(null, "main.py")).toBe("text");
      expect(classifyForPreview(null, "main.go")).toBe("text");
      expect(classifyForPreview(null, "App.java")).toBe("text");
      expect(classifyForPreview(null, "hello.c")).toBe("text");
      expect(classifyForPreview(null, "hello.cpp")).toBe("text");
    });

    it("classifies config file extensions as text", () => {
      expect(classifyForPreview(null, "config.yaml")).toBe("text");
      expect(classifyForPreview(null, "config.yml")).toBe("text");
      expect(classifyForPreview(null, "config.toml")).toBe("text");
      expect(classifyForPreview(null, "config.ini")).toBe("text");
      expect(classifyForPreview(null, ".env")).toBe("text");
    });

    it("classifies shell scripts as text", () => {
      expect(classifyForPreview(null, "script.sh")).toBe("text");
      expect(classifyForPreview(null, "script.bash")).toBe("text");
      expect(classifyForPreview(null, "script.zsh")).toBe("text");
    });

    it("classifies image extensions as image", () => {
      expect(classifyForPreview(null, "photo.png")).toBe("image");
      expect(classifyForPreview(null, "photo.jpg")).toBe("image");
      expect(classifyForPreview(null, "photo.jpeg")).toBe("image");
      expect(classifyForPreview(null, "anim.gif")).toBe("image");
      expect(classifyForPreview(null, "pic.webp")).toBe("image");
      expect(classifyForPreview(null, "pic.bmp")).toBe("image");
      expect(classifyForPreview(null, "icon.ico")).toBe("image");
    });

    it("classifies unknown extensions as binary", () => {
      expect(classifyForPreview(null, "data.bin")).toBe("binary");
      expect(classifyForPreview(null, "archive.zip")).toBe("binary");
      expect(classifyForPreview(null, "app.exe")).toBe("binary");
      expect(classifyForPreview(null, "video.mp4")).toBe("binary");
    });

    it("classifies files with no extension as binary when unrecognized", () => {
      expect(classifyForPreview(null, "LICENSE")).toBe("binary");
    });

    it("classifies Makefile as text via extension fallback", () => {
      // "Makefile" has no dot, so split(".").pop() returns "makefile" (lowercased),
      // which is in the textExts list
      expect(classifyForPreview(null, "Makefile")).toBe("text");
    });
  });

  describe("MIME takes priority over extension", () => {
    it("uses MIME when it disagrees with extension", () => {
      // SVG has image MIME but .svg is in text extensions — MIME wins
      expect(classifyForPreview("image/svg+xml", "icon.svg")).toBe("image");
    });
  });
});

describe("MAX_PREVIEW_SIZE", () => {
  it("is 10 MB", () => {
    expect(MAX_PREVIEW_SIZE).toBe(10 * 1024 * 1024);
  });
});

describe("isArchive", () => {
  it("recognizes .zip files", () => {
    expect(isArchive("archive.zip")).toBe(true);
    expect(isArchive("ARCHIVE.ZIP")).toBe(true);
    expect(isArchive("my-file.Zip")).toBe(true);
  });

  it("recognizes .tar.gz files", () => {
    expect(isArchive("project.tar.gz")).toBe(true);
    expect(isArchive("PROJECT.TAR.GZ")).toBe(true);
  });

  it("recognizes .tgz files", () => {
    expect(isArchive("backup.tgz")).toBe(true);
    expect(isArchive("BACKUP.TGZ")).toBe(true);
  });

  it("rejects non-archive files", () => {
    expect(isArchive("readme.txt")).toBe(false);
    expect(isArchive("image.png")).toBe(false);
    expect(isArchive("app.exe")).toBe(false);
    expect(isArchive("data.tar")).toBe(false);
    expect(isArchive("file.gz")).toBe(false);
  });
});
