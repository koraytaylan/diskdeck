/**
 * @file Add / Edit Disk Dialog
 *
 * Modal dialog for creating a new storage backend or editing an existing one.
 * Supports all backend types (local, S3, GCS, Azure, SFTP, FTP) with
 * type-specific configuration fields.
 *
 * **Create vs. Edit mode:**
 * - **Create** (no `editDisk` prop): All fields start empty. The disk type
 *   selector is interactive. On submit, `addDisk()` is called and the new
 *   disk is auto-selected + navigated to root.
 * - **Edit** (`editDisk` prop provided): Fields are pre-populated from the
 *   existing config via a `createEffect`. The type selector is disabled
 *   (disk type cannot change after creation). On submit, `editDisk()` is called.
 *
 * **Validation:**
 * Client-side validation ensures required fields are filled before submission.
 * Backend validation errors (e.g. invalid credentials) are caught and displayed
 * in the error area.
 *
 * **Lifecycle:**
 * - `reset()` clears all fields and is called on close and after successful submit.
 * - The `createEffect` on `props.editDisk` populates fields when entering edit mode.
 * - Backdrop click and Escape key both close the dialog.
 *
 * @module components/disk/AddDiskDialog
 */

import { createSignal, createEffect, on, Show, type Component } from "solid-js";
import { open } from "@tauri-apps/plugin-dialog";
import { X } from "lucide-solid";
import type { DiskConfig, DiskType } from "../../lib/types";
import { useDisk } from "../../contexts/DiskContext";
import { useFile } from "../../contexts/FileContext";
import { useTab } from "../../contexts/TabContext";
import styles from "./AddDiskDialog.module.css";

/**
 * Modal dialog for adding or editing a disk configuration.
 *
 * @param props.open     - Whether the dialog is visible.
 * @param props.onClose  - Callback to close the dialog.
 * @param props.editDisk - If provided, the dialog opens in edit mode with pre-filled fields.
 */
export const AddDiskDialog: Component<{
  open: boolean;
  onClose: () => void;
  editDisk?: DiskConfig;
}> = (props) => {
  const { addDisk, editDisk, selectDisk } = useDisk();
  const { navigate } = useFile();
  const { openBrowserTab } = useTab();
  const [diskType, setDiskType] = createSignal<DiskType>("local");
  const [name, setName] = createSignal("");
  // Local fields
  const [root, setRoot] = createSignal("");
  // S3 fields
  const [bucket, setBucket] = createSignal("");
  const [region, setRegion] = createSignal("us-east-1");
  const [accessKeyId, setAccessKeyId] = createSignal("");
  const [secretAccessKey, setSecretAccessKey] = createSignal("");
  // Azure fields
  const [azureAccount, setAzureAccount] = createSignal("");
  const [azureAccessKey, setAzureAccessKey] = createSignal("");
  const [azureContainer, setAzureContainer] = createSignal("");
  // GCS fields
  const [gcsBucket, setGcsBucket] = createSignal("");
  const [gcsCredentialsJson, setGcsCredentialsJson] = createSignal("");
  // SFTP / FTP shared fields
  const [host, setHost] = createSignal("");
  const [port, setPort] = createSignal("");
  const [username, setUsername] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [ftpTls, setFtpTls] = createSignal(false);
  // SFTP SSH key auth
  const [sftpAuthMode, setSftpAuthMode] = createSignal<"password" | "key">("password");
  const [keyPath, setKeyPath] = createSignal("");
  const [error, setError] = createSignal("");
  const [submitting, setSubmitting] = createSignal(false);

  const isEditing = () => !!props.editDisk;

  /** Tracks whether the user has manually typed into the name field.
   *  When false, browsing auto-fills the name with the selected folder name.
   *  When true, the user's manual input is preserved across browse actions. */
  let nameManuallySet = false;

  /** Opens a native folder picker and sets the root path.
   *  Auto-fills the name field with the folder name unless the user has
   *  manually typed a name. */
  const handleBrowse = async () => {
    const selected = await open({ directory: true, multiple: false, title: "Select Root Folder" });
    if (selected) {
      const path = selected as string;
      setRoot(path);
      if (!nameManuallySet) {
        const folderName = path.split("/").filter(Boolean).pop() ?? path;
        setName(folderName);
      }
    }
  };

  const reset = () => {
    setDiskType("local");
    setName("");
    nameManuallySet = false;
    setRoot("");
    setBucket("");
    setRegion("us-east-1");
    setAccessKeyId("");
    setSecretAccessKey("");
    setAzureAccount("");
    setAzureAccessKey("");
    setAzureContainer("");
    setGcsBucket("");
    setGcsCredentialsJson("");
    setHost("");
    setPort("");
    setUsername("");
    setPassword("");
    setFtpTls(false);
    setSftpAuthMode("password");
    setKeyPath("");
    setError("");
    setSubmitting(false);
  };

  // Populate fields when editing
  createEffect(
    on(
      () => props.editDisk,
      (disk) => {
        if (disk) {
          setDiskType(disk.disk_type);
          setName(disk.name);
          const cfg = disk.config as Record<string, string>;
          if (disk.disk_type === "local") {
            setRoot(cfg.root ?? "");
          } else if (disk.disk_type === "s3") {
            setBucket(cfg.bucket ?? "");
            setRegion(cfg.region ?? "us-east-1");
            setAccessKeyId(cfg.access_key_id ?? "");
            setSecretAccessKey(cfg.secret_access_key ?? "");
          } else if (disk.disk_type === "gcs") {
            setGcsBucket(cfg.bucket ?? "");
            setGcsCredentialsJson(cfg.credentials_json ?? "");
          } else if (disk.disk_type === "azure") {
            setAzureAccount(cfg.account ?? "");
            setAzureAccessKey(cfg.access_key ?? "");
            setAzureContainer(cfg.container ?? "");
          } else if (disk.disk_type === "sftp") {
            setHost(cfg.host ?? "");
            setPort(cfg.port ?? "22");
            setUsername(cfg.username ?? "");
            setPassword(cfg.password ?? "");
            if (cfg.key_path) {
              setSftpAuthMode("key");
              setKeyPath(cfg.key_path);
            } else {
              setSftpAuthMode("password");
              setKeyPath("");
            }
          } else if (disk.disk_type === "ftp") {
            setHost(cfg.host ?? "");
            setPort(cfg.port ?? "21");
            setUsername(cfg.username ?? "");
            setPassword(cfg.password ?? "");
            setFtpTls(cfg.tls === "true");
          }
          setError("");
          setSubmitting(false);
        }
      },
    ),
  );

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    const diskName = name().trim();
    if (!diskName) {
      setError("Name is required");
      return;
    }

    let config: Record<string, unknown>;

    const dt = diskType();
    if (dt === "local") {
      const diskRoot = root().trim();
      if (!diskRoot) { setError("Root path is required"); return; }
      config = { root: diskRoot };
    } else if (dt === "s3") {
      const b = bucket().trim();
      const r = region().trim();
      const ak = accessKeyId().trim();
      const sk = secretAccessKey().trim();
      if (!b) { setError("Bucket is required"); return; }
      if (!r) { setError("Region is required"); return; }
      if (!ak) { setError("Access Key ID is required"); return; }
      if (!sk) { setError("Secret Access Key is required"); return; }
      config = { bucket: b, region: r, access_key_id: ak, secret_access_key: sk };
    } else if (dt === "gcs") {
      const b = gcsBucket().trim();
      const cj = gcsCredentialsJson().trim();
      if (!b) { setError("Bucket is required"); return; }
      if (!cj) { setError("Service Account JSON is required"); return; }
      config = { bucket: b, credentials_json: cj };
    } else if (dt === "azure") {
      const acc = azureAccount().trim();
      const ak = azureAccessKey().trim();
      const ctr = azureContainer().trim();
      if (!acc) { setError("Account is required"); return; }
      if (!ak) { setError("Access Key is required"); return; }
      if (!ctr) { setError("Container is required"); return; }
      config = { account: acc, access_key: ak, container: ctr };
    } else if (dt === "sftp") {
      const h = host().trim();
      const u = username().trim();
      if (!h) { setError("Host is required"); return; }
      if (!u) { setError("Username is required"); return; }
      const defaultPort = "22";
      config = { host: h, port: port().trim() || defaultPort, username: u };
      if (sftpAuthMode() === "key") {
        const kp = keyPath().trim();
        if (!kp) { setError("Key file path is required"); return; }
        config.key_path = kp;
      } else {
        const p = password().trim();
        if (!p) { setError("Password is required"); return; }
        config.password = p;
      }
    } else if (dt === "ftp") {
      const h = host().trim();
      const u = username().trim();
      const p = password().trim();
      if (!h) { setError("Host is required"); return; }
      if (!u) { setError("Username is required"); return; }
      if (!p) { setError("Password is required"); return; }
      const defaultPort = "21";
      config = { host: h, port: port().trim() || defaultPort, username: u, password: p };
      config.tls = ftpTls() ? "true" : "false";
    } else {
      setError("Unknown disk type");
      return;
    }

    setSubmitting(true);
    setError("");
    try {
      if (isEditing()) {
        await editDisk(props.editDisk!.id, diskName, config);
      } else {
        const disk = await addDisk(diskName, diskType(), config);
        selectDisk(disk.id);
        openBrowserTab(disk.id, disk.name);
        navigate(disk.id, "/");
      }
      reset();
      props.onClose();
    } catch (e: unknown) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  };

  const handleBackdropClick = (e: MouseEvent) => {
    if (e.target === e.currentTarget) {
      reset();
      props.onClose();
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      reset();
      props.onClose();
    }
  };

  return (
    <Show when={props.open}>
      <div
        class={styles.backdrop}
        onClick={handleBackdropClick}
        onKeyDown={handleKeyDown}
      >
        <div class={styles.dialog}>
          <div class={styles.header}>
            <span>{isEditing() ? "Edit Disk" : "Add Disk"}</span>
            <button
              class={styles.closeButton}
              onClick={() => { reset(); props.onClose(); }}
            >
              <X size={14} />
            </button>
          </div>

          <form class={styles.form} onSubmit={handleSubmit}>
            <div class={styles.typeSelector}>
              {([
                { category: "Local", types: [{ id: "local" as const, label: "Local Disk" }] },
                { category: "Cloud Storage", types: [
                  { id: "s3" as const, label: "AWS S3" },
                  { id: "gcs" as const, label: "Google Cloud" },
                  { id: "azure" as const, label: "Azure Blob" },
                ]},
                { category: "Remote Servers", types: [
                  { id: "sftp" as const, label: "SFTP" },
                  { id: "ftp" as const, label: "FTP / FTPS" },
                ]},
              ]).map((group) => (
                <div class={styles.typeGroup}>
                  <span class={styles.typeGroupLabel}>{group.category}</span>
                  <div class={styles.typeGrid}>
                    {group.types.map((t) => (
                      <button
                        type="button"
                        class={styles.typeTile}
                        classList={{
                          [styles.typeTileActive]: diskType() === t.id,
                          [styles.typeTileDisabled]: isEditing(),
                        }}
                        onClick={() => !isEditing() && setDiskType(t.id)}
                        disabled={isEditing()}
                      >
                        {t.label}
                      </button>
                    ))}
                  </div>
                </div>
              ))}
            </div>

            <label class={styles.field}>
              <span class={styles.label}>Name</span>
              <input
                class={styles.input}
                type="text"
                placeholder={{ local: "My Files", s3: "My S3 Bucket", gcs: "My GCS Bucket", azure: "My Azure Container", sftp: "My SFTP Server", ftp: "My FTP Server" }[diskType()]}
                value={name()}
                onInput={(e) => { setName(e.currentTarget.value); nameManuallySet = true; }}
                autofocus
              />
            </label>

            <Show when={diskType() === "local"}>
              <label class={styles.field}>
                <span class={styles.label}>Root Path</span>
                <div class={styles.pathRow}>
                  <input
                    class={styles.input}
                    type="text"
                    placeholder="/Users/username/Documents"
                    value={root()}
                    onInput={(e) => setRoot(e.currentTarget.value)}
                  />
                  <button
                    type="button"
                    class={styles.browseButton}
                    onClick={handleBrowse}
                  >
                    Browse
                  </button>
                </div>
              </label>
            </Show>

            <Show when={diskType() === "s3"}>
              <label class={styles.field}>
                <span class={styles.label}>Bucket</span>
                <input
                  class={styles.input}
                  type="text"
                  placeholder="my-bucket"
                  value={bucket()}
                  onInput={(e) => setBucket(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Region</span>
                <input
                  class={styles.input}
                  type="text"
                  placeholder="us-east-1"
                  value={region()}
                  onInput={(e) => setRegion(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Access Key ID</span>
                <input
                  class={styles.input}
                  type="text"
                  placeholder="AKIA..."
                  value={accessKeyId()}
                  onInput={(e) => setAccessKeyId(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Secret Access Key</span>
                <input
                  class={styles.input}
                  type="password"
                  placeholder="Secret key"
                  value={secretAccessKey()}
                  onInput={(e) => setSecretAccessKey(e.currentTarget.value)}
                />
              </label>
            </Show>

            <Show when={diskType() === "gcs"}>
              <label class={styles.field}>
                <span class={styles.label}>Bucket</span>
                <input
                  class={styles.input}
                  type="text"
                  placeholder="my-gcs-bucket"
                  value={gcsBucket()}
                  onInput={(e) => setGcsBucket(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Service Account JSON</span>
                <textarea
                  class={styles.input}
                  placeholder='Paste the contents of your service account key JSON file'
                  rows={6}
                  value={gcsCredentialsJson()}
                  onInput={(e) => setGcsCredentialsJson(e.currentTarget.value)}
                />
              </label>
            </Show>

            <Show when={diskType() === "azure"}>
              <label class={styles.field}>
                <span class={styles.label}>Account Name</span>
                <input
                  class={styles.input}
                  type="text"
                  placeholder="mystorageaccount"
                  value={azureAccount()}
                  onInput={(e) => setAzureAccount(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Access Key</span>
                <input
                  class={styles.input}
                  type="password"
                  placeholder="Access key"
                  value={azureAccessKey()}
                  onInput={(e) => setAzureAccessKey(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Container</span>
                <input
                  class={styles.input}
                  type="text"
                  placeholder="my-container"
                  value={azureContainer()}
                  onInput={(e) => setAzureContainer(e.currentTarget.value)}
                />
              </label>
            </Show>

            <Show when={diskType() === "sftp"}>
              <label class={styles.field}>
                <span class={styles.label}>Host</span>
                <input
                  class={styles.input}
                  type="text"
                  placeholder="example.com"
                  value={host()}
                  onInput={(e) => setHost(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Port</span>
                <input
                  class={styles.input}
                  type="text"
                  placeholder="22"
                  value={port()}
                  onInput={(e) => setPort(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Username</span>
                <input
                  class={styles.input}
                  type="text"
                  placeholder="user"
                  value={username()}
                  onInput={(e) => setUsername(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Authentication</span>
                <div class={styles.typeToggle}>
                  <button
                    type="button"
                    class={styles.typeButton}
                    classList={{ [styles.typeActive]: sftpAuthMode() === "password" }}
                    onClick={() => setSftpAuthMode("password")}
                  >
                    Password
                  </button>
                  <button
                    type="button"
                    class={styles.typeButton}
                    classList={{ [styles.typeActive]: sftpAuthMode() === "key" }}
                    onClick={() => setSftpAuthMode("key")}
                  >
                    SSH Key
                  </button>
                </div>
              </label>
              <Show when={sftpAuthMode() === "password"}>
                <label class={styles.field}>
                  <span class={styles.label}>Password</span>
                  <input
                    class={styles.input}
                    type="password"
                    placeholder="Password"
                    value={password()}
                    onInput={(e) => setPassword(e.currentTarget.value)}
                  />
                </label>
              </Show>
              <Show when={sftpAuthMode() === "key"}>
                <label class={styles.field}>
                  <span class={styles.label}>Key File Path</span>
                  <input
                    class={styles.input}
                    type="text"
                    placeholder="~/.ssh/id_rsa"
                    value={keyPath()}
                    onInput={(e) => setKeyPath(e.currentTarget.value)}
                  />
                </label>
              </Show>
            </Show>

            <Show when={diskType() === "ftp"}>
              <label class={styles.field}>
                <span class={styles.label}>Host</span>
                <input
                  class={styles.input}
                  type="text"
                  placeholder="example.com"
                  value={host()}
                  onInput={(e) => setHost(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Port</span>
                <input
                  class={styles.input}
                  type="text"
                  placeholder="21"
                  value={port()}
                  onInput={(e) => setPort(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Username</span>
                <input
                  class={styles.input}
                  type="text"
                  placeholder="user"
                  value={username()}
                  onInput={(e) => setUsername(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Password</span>
                <input
                  class={styles.input}
                  type="password"
                  placeholder="Password"
                  value={password()}
                  onInput={(e) => setPassword(e.currentTarget.value)}
                />
              </label>
              <label class={styles.field}>
                <span class={styles.label}>Use TLS (FTPS)</span>
                <input
                  type="checkbox"
                  checked={ftpTls()}
                  onChange={(e) => setFtpTls(e.currentTarget.checked)}
                />
              </label>
            </Show>

            <Show when={error()}>
              <div class={styles.error}>{error()}</div>
            </Show>

            <div class={styles.actions}>
              <button
                type="button"
                class={styles.cancelButton}
                onClick={() => { reset(); props.onClose(); }}
              >
                Cancel
              </button>
              <button
                type="submit"
                class={styles.submitButton}
                disabled={submitting()}
              >
                {submitting()
                  ? isEditing() ? "Saving..." : "Adding..."
                  : isEditing() ? "Save" : "Add Disk"}
              </button>
            </div>
          </form>
        </div>
      </div>
    </Show>
  );
};
