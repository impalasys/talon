import { createHash, randomUUID } from 'crypto';
import { mkdir, readFile, rename, unlink, writeFile } from 'fs/promises';
import path from 'path';
import { NextRequest, NextResponse } from 'next/server';

export const runtime = 'nodejs';

const DEFAULT_OBJECT_STORE_PATH = '/data/talon/objects';
const MAX_IMAGE_BYTES = 20 * 1024 * 1024;
const SUPPORTED_IMAGE_TYPES = new Set(['image/png', 'image/jpeg', 'image/gif', 'image/webp']);

function safeSegment(value: string, fallback: string) {
  const segment = value
    .trim()
    .replace(/[^A-Za-z0-9._-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 96);
  return segment && segment !== '.' && segment !== '..' ? segment : fallback;
}

function extensionForMediaType(mediaType: string) {
  switch (mediaType) {
    case 'image/jpeg':
      return '.jpg';
    case 'image/gif':
      return '.gif';
    case 'image/webp':
      return '.webp';
    case 'image/png':
    default:
      return '.png';
  }
}

function objectDataPath(root: string, key: string) {
  const resolvedRoot = path.resolve(root);
  const resolvedPath = path.resolve(resolvedRoot, key);
  if (!resolvedPath.startsWith(`${resolvedRoot}${path.sep}`)) {
    throw new Error('invalid object key');
  }
  return resolvedPath;
}

function metadataPath(dataPath: string) {
  return path.join(path.dirname(dataPath), `${path.basename(dataPath)}.metadata.json`);
}

async function readObjectMetadata(dataPath: string) {
  try {
    const bytes = await readFile(metadataPath(dataPath), 'utf8');
    const metadata = JSON.parse(bytes);
    return metadata && typeof metadata === 'object' ? metadata as Record<string, unknown> : {};
  } catch {
    return {};
  }
}

async function removeIfExists(filePath: string) {
  try {
    await unlink(filePath);
  } catch (err: any) {
    if (err?.code !== 'ENOENT') {
      throw err;
    }
  }
}

async function writeObjectWithMetadata(
  dataPath: string,
  metaPath: string,
  bytes: Buffer,
  metadata: Record<string, unknown>,
) {
  const tempSuffix = `.tmp-${randomUUID()}`;
  const dataTempPath = `${dataPath}${tempSuffix}`;
  const metaTempPath = `${metaPath}${tempSuffix}`;
  let dataCommitted = false;

  try {
    await writeFile(dataTempPath, bytes);
    await writeFile(metaTempPath, JSON.stringify(metadata));
    await rename(dataTempPath, dataPath);
    dataCommitted = true;
    await rename(metaTempPath, metaPath);
  } catch (err) {
    await Promise.all([
      removeIfExists(dataTempPath),
      removeIfExists(metaTempPath),
      dataCommitted ? removeIfExists(dataPath) : Promise.resolve(),
    ]);
    throw err;
  }
}

export async function GET(request: NextRequest) {
  const key = request.nextUrl.searchParams.get('key');
  if (!key) {
    return NextResponse.json({ error: 'key is required' }, { status: 400 });
  }

  let dataPath: string;
  try {
    dataPath = objectDataPath(process.env.TALON_OBJECT_STORE_PATH || DEFAULT_OBJECT_STORE_PATH, key);
  } catch {
    return NextResponse.json({ error: 'invalid object key' }, { status: 400 });
  }

  try {
    const [bytes, metadata] = await Promise.all([
      readFile(dataPath),
      readObjectMetadata(dataPath),
    ]);
    const mediaType = typeof metadata.media_type === 'string' && metadata.media_type
      ? metadata.media_type
      : 'application/octet-stream';
    return new NextResponse(new Uint8Array(bytes), {
      headers: {
        'content-type': mediaType,
        'cache-control': 'private, max-age=300',
      },
    });
  } catch {
    return NextResponse.json({ error: 'object not found' }, { status: 404 });
  }
}

export async function POST(request: NextRequest) {
  let form: FormData;
  try {
    form = await request.formData();
  } catch {
    return NextResponse.json({ error: 'multipart form data is required' }, { status: 400 });
  }
  const file = form.get('file');
  if (!(file instanceof File)) {
    return NextResponse.json({ error: 'file is required' }, { status: 400 });
  }
  if (!SUPPORTED_IMAGE_TYPES.has(file.type)) {
    return NextResponse.json({ error: 'unsupported image type' }, { status: 400 });
  }
  if (file.size > MAX_IMAGE_BYTES) {
    return NextResponse.json({ error: 'image is too large' }, { status: 413 });
  }

  const namespace = safeSegment(String(form.get('namespace') || 'default'), 'default');
  const agent = safeSegment(String(form.get('agent') || 'default'), 'default');
  const sessionId = safeSegment(String(form.get('sessionId') || 'session'), 'session');
  const originalName = file.name || `image${extensionForMediaType(file.type)}`;
  const extension = path.extname(originalName) || extensionForMediaType(file.type);
  const key = [
    'sessions',
    namespace,
    agent,
    sessionId,
    'uploads',
    `${Date.now()}-${randomUUID()}${extension}`,
  ].join('/');

  const bytes = Buffer.from(await file.arrayBuffer());
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  const root = process.env.TALON_OBJECT_STORE_PATH || DEFAULT_OBJECT_STORE_PATH;
  const dataPath = objectDataPath(root, key);
  const metaPath = metadataPath(dataPath);
  const object = {
    key,
    mediaType: file.type,
    sizeBytes: bytes.length,
    sha256,
    filename: originalName,
    metadata: {
      source: 'sightline',
    },
  };
  const metadata = {
    media_type: object.mediaType,
    size_bytes: object.sizeBytes,
    sha256: object.sha256,
    filename: object.filename,
    metadata: object.metadata,
  };

  await mkdir(path.dirname(dataPath), { recursive: true });
  await writeObjectWithMetadata(dataPath, metaPath, bytes, metadata);

  return NextResponse.json(object);
}
